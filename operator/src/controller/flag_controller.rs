use crate::v1alpha1::{Flag, FlagFormat, FlagStatus};
use crate::{Context, Result, upsert_condition};
use jiff::Timestamp;
use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use rand::Rng;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

use kube::Resource;
use kube::core::object::HasStatus;
use kube::runtime::controller::Action;

use crate::Controller;

pub struct FlagController {}

impl Controller<Flag> for FlagController {
    const NAME: &'static str = "flag-controller";

    async fn reconcile(flag: Arc<Flag>, ctx: Arc<Context>) -> Result<Action> {
        let eflag = get_evaluated_flag(&flag);

        let eflag = match eflag {
            None => {
                info!("Creating flag");
                return create_flag(&flag, &ctx).await;
            }
            Some(eflag) => eflag,
        };

        if !validate_flag(&flag, &eflag) {
            info!("Recreating flag");
            return create_flag(&flag, &ctx).await;
        }

        // no op: Previous flag matches new format
        Ok(Action::await_change())
    }
}

fn get_evaluated_flag(flag: &Flag) -> Option<String> {
    flag.status().map(|stat| stat.flag.clone())
}

fn default_conditions(flag: &Flag) -> Vec<Condition> {
    let flag_gen = flag.meta().generation;
    vec![
        Condition {
            last_transition_time: Timestamp::now().into(),
            message: "Flag config is not validated".into(),
            observed_generation: flag_gen,
            reason: "FlagValid".into(),
            status: "False".into(),
            type_: "Validated".into(),
        },
        Condition {
            last_transition_time: Timestamp::now().into(),
            message: "Flag is not generated".into(),
            observed_generation: flag_gen,
            reason: "FlagGenerated".into(),
            status: "False".into(),
            type_: "Generated".into(),
        },
    ]
}

async fn create_flag(flag: &Flag, ctx: &Context) -> Result<Action> {
    let mut status = flag.status.clone().unwrap_or_else(|| FlagStatus {
        flag: String::new(),
        conditions: Vec::new(),
    });

    if status.conditions.is_empty() {
        status.conditions = default_conditions(flag);
    }

    let flag_validation_condition = gen_flag_validation_condition(flag);
    let flag_validation_condition_ok = flag_validation_condition.reason == "FlagValid"
        && flag_validation_condition.status == "True";
    upsert_condition(&mut status.conditions, flag_validation_condition);

    if !flag_validation_condition_ok {
        //Reset flag
        status.flag = String::new();
        FlagController::patch_status_namespaced(flag, ctx, status).await?;
        return Ok(Action::requeue(Duration::from_secs(
            FlagController::ERROR_RECONCILE_REQUEUE_TIME,
        )));
    }

    status.flag = generate_flag(flag);

    let flag_gen = flag.meta().generation;
    upsert_condition(
        &mut status.conditions,
        Condition {
            last_transition_time: Timestamp::now().into(),
            message: "Flag is generated".into(),
            observed_generation: flag_gen,
            reason: "FlagGenerated".into(),
            status: "True".into(),
            type_: "Generated".into(),
        },
    );

    FlagController::patch_status_namespaced(flag, ctx, status).await?;
    Ok(Action::await_change())
}

fn gen_flag_validation_condition(flag: &Flag) -> Condition {
    let flag_gen = flag.meta().generation;
    let flag_spec = &flag.spec;
    if flag_spec.prefix.trim().is_empty() {
        return Condition {
            last_transition_time: Timestamp::now().into(),
            message: "Flag prefix cant be empty".into(),
            observed_generation: flag_gen,
            reason: "PrefixEmpty".into(),
            status: "False".into(),
            type_: "Validated".into(),
        };
    }

    if flag_spec.body.trim().is_empty() {
        return Condition {
            last_transition_time: Timestamp::now().into(),
            message: "Flag body cant be empty".into(),
            observed_generation: flag_gen,
            reason: "BodyEmpty".into(),
            status: "False".into(),
            type_: "Validated".into(),
        };
    }

    Condition {
        last_transition_time: Timestamp::now().into(),
        message: "Flag config is validated".into(),
        observed_generation: flag_gen,
        reason: "FlagValid".into(),
        status: "True".into(),
        type_: "Validated".into(),
    }
}

fn generate_flag(flag: &Flag) -> String {
    let prefix = &flag.spec.prefix;
    let mut body = flag.spec.body.to_owned();

    if let Some(rand) = &flag.spec.random {
        if let Some(c) = rand.concat {
            body.push(c);
        }

        let mut alphabet = String::new();
        if rand.digits {
            alphabet.push_str("0123456789");
        }
        if rand.upper {
            alphabet.push_str("ABCDEFGHIJKLMNOPQRSTUVWXYZ");
        }
        if rand.lower {
            alphabet.push_str("abcdefghijklmnopqrstuvwxyz");
        }
        if rand.lower {
            alphabet.push_str(&rand.special);
        }

        let mut rng = rand::rng();
        let value: String = (0..rand.len)
            .map(|_| {
                let idx = rng.random_range(0..alphabet.len());
                alphabet.as_bytes()[idx] as char
            })
            .collect();
        debug_assert_eq!(value.len(), rand.len as usize);
        body.push_str(&value);
    }

    match flag.spec.format {
        FlagFormat::Curly => format!("{prefix}{{{body}}}"),
        FlagFormat::Round => format!("{prefix}({body})"),
        FlagFormat::Square => format!("{prefix}[{body}]"),
        FlagFormat::Angle => format!("{prefix}<{body}>"),
        FlagFormat::Underscore => format!("{prefix}_{body}"),
    }
}

fn validate_flag(flag: &Flag, flag_str: &str) -> bool {
    let prefix = &flag.spec.prefix;
    let body = &flag.spec.body;

    // TODO: Regex check after instancing is added
    let (mut start, end) = match flag.spec.format {
        FlagFormat::Curly => (format!("{prefix}{{{body}"), "}"),
        FlagFormat::Round => (format!("{prefix}({body}"), ")"),
        FlagFormat::Square => (format!("{prefix}[{body}"), "]"),
        FlagFormat::Angle => (format!("{prefix}<{body}"), ">"),
        FlagFormat::Underscore => (format!("{prefix}_{body}"), ""),
    };

    if let Some(Some(c)) = flag.spec.random.as_ref().map(|r| r.concat) {
        start.push(c);
    }

    let rand_len = flag.spec.random.as_ref().map(|r| r.len).unwrap_or_default() as usize;

    if flag_str.len() != start.len() + end.len() + rand_len {
        return false;
    }

    if !flag_str.starts_with(&start) || !flag_str.ends_with(end) {
        return false;
    }

    if let Some(rand) = &flag.spec.random {
        let rand_str = &flag_str[start.len()..start.len() + rand.len as usize];
        debug_assert_eq!(flag_str, &format!("{start}{rand_str}{end}"));
        for c in rand_str.chars() {
            if rand.digits && c.is_digit(10) {
                continue;
            }

            if rand.upper && c.is_uppercase() {
                continue;
            }

            if rand.lower && c.is_lowercase() {
                continue;
            }

            if rand.special.contains(c) {
                continue;
            }

            return false;
        }
    }

    true
}
