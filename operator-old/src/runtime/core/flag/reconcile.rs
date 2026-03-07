use crate::crd::v1alpha1::{FlagFormat, FlagStatus};
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    crd::v1alpha1::Flag,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use kube::ResourceExt;
use kube::runtime::controller::Action;
use rand::Rng;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

pub struct FlagReconciler {}
impl Reconciler for FlagReconciler {
    const NAME: &'static str = "flag-controller";
    type K = Flag;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Flag>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let flag = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        if Self::validate_spec(flag, &mut status) {
            if !Self::validate_flag(flag, &mut status) {
                Self::generate(flag, &mut status);
                Self::validate_flag(flag, &mut status);
            }
        } else {
            status.flag = None;
        }

        let conditions_true = status.conditions.all_true();

        if flag.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(flag, ctx.client.clone(), status).await?;
        }

        if conditions_true {
            Ok(Action::await_change())
        } else {
            Ok(Action::requeue(Duration::from_secs(
                Self::ERROR_RECONCILE_REQUEUE_TIME,
            )))
        }
    }
}

impl FlagReconciler {
    const GENERATE_CONDITION_TYPE: &'static str = "GenerateFlag";
    const GENERATE_REASON_SUCCEEDED: &'static str = "Generated";
    const GENERATE_REASON_GENERATING: &'static str = "Generating";

    fn generate(flag: &Flag, status: &mut FlagStatus) {
        status.conditions.set(
            Self::GENERATE_CONDITION_TYPE,
            ConditionStatus::False,
            Self::GENERATE_REASON_GENERATING,
            "",
        );

        let flag = generate_flag(flag);

        status.conditions.set(
            Self::GENERATE_CONDITION_TYPE,
            ConditionStatus::True,
            Self::GENERATE_REASON_SUCCEEDED,
            "",
        );

        status.flag = Some(flag.clone());
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
        if !rand.special.is_empty() {
            // Discard duplicated chars
            let mut set: HashSet<char> = alphabet.chars().collect();
            for c in rand.special.chars() {
                if set.insert(c) {
                    alphabet.push(c);
                }
            }
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

impl FlagReconciler {
    const VALIDATE_SPEC_CONDITION_TYPE: &'static str = "ValidateSpec";
    const VALIDATE_SPEC_REASON_SUCCEEDED: &'static str = "Validated";
    const VALIDATE_SPEC_REASON_FAILED: &'static str = "Failed";

    const VALIDATE_SPEC_MESSAGE_PREFIX_EMPTY: &'static str = "Flag prefix is empty";
    const VALIDATE_SPEC_MESSAGE_PREFIX_NOT_ALPHANUM: &'static str =
        "Flag prefix can only contain letters and numbers";
    const VALIDATE_SPEC_MESSAGE_PREFIX_BAD_LEN: &'static str =
        "Flag prefix must be between 3 and 6 characters long";

    const VALIDATE_SPEC_MESSAGE_BODY_EMPTY: &'static str = "Flag body is empty";
    const VALIDATE_SPEC_MESSAGE_BODY_BAD_LEN: &'static str =
        "Flag body cant be larger than 128 chars";

    const VALIDATE_SPEC_MESSAGE_RANDOM_ZERO_LEN: &'static str = "Flag random len must be non-zero";
    const VALIDATE_SPEC_MESSAGE_RANDOM_BAD_LEN: &'static str =
        "Flag random len cant be larger than 64";
    const VALIDATE_SPEC_MESSAGE_RANDOM_NOT_DISTINCT: &'static str =
        "Flag random special must contain unique characters";

    fn validate_spec(flag: &Flag, status: &mut FlagStatus) -> bool {
        let spec = &flag.spec;

        if spec.prefix.is_empty() {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_PREFIX_EMPTY,
            );
            return false;
        }

        if !spec.prefix.chars().all(|c| c.is_alphanumeric()) {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_PREFIX_NOT_ALPHANUM,
            );
            return false;
        }

        if !(3..=6usize).contains(&spec.prefix.chars().count()) {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_PREFIX_BAD_LEN,
            );
            return false;
        }

        if spec.body.is_empty() {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_BODY_EMPTY,
            );
            return false;
        }

        if spec.body.chars().count() >= 128usize {
            status.conditions.set(
                Self::VALIDATE_SPEC_CONDITION_TYPE,
                ConditionStatus::False,
                Self::VALIDATE_SPEC_REASON_FAILED,
                Self::VALIDATE_SPEC_MESSAGE_BODY_BAD_LEN,
            );
            return false;
        }

        if let Some(random) = &spec.random {
            if random.len == 0 {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_RANDOM_ZERO_LEN,
                );
                return false;
            }

            if random.len >= 64u8 {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_RANDOM_BAD_LEN,
                );
                return false;
            }

            if has_duplicate_chars(&random.special) {
                status.conditions.set(
                    Self::VALIDATE_SPEC_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_SPEC_REASON_FAILED,
                    Self::VALIDATE_SPEC_MESSAGE_RANDOM_NOT_DISTINCT,
                );
                return false;
            }
        }

        status.conditions.set(
            Self::VALIDATE_SPEC_CONDITION_TYPE,
            ConditionStatus::True,
            Self::VALIDATE_SPEC_REASON_SUCCEEDED,
            "",
        );
        true
    }
}

impl FlagReconciler {
    const VALIDATE_FLAG_CONDITION_TYPE: &'static str = "ValidateFlag";
    const VALIDATE_FLAG_REASON_SUCCEEDED: &'static str = "Validated";
    const VALIDATE_FLAG_REASON_FAILED: &'static str = "Failed";

    const VALIDATE_FLAG_MESSAGE_EMPTY: &'static str = "Flag is empty";
    const VALIDATE_FLAG_MESSAGE_NOT_MATCHING: &'static str = "Flag is not matching defined rules";

    fn validate_flag(flag: &Flag, status: &mut FlagStatus) -> bool {
        match &status.flag {
            None => {
                status.conditions.set(
                    Self::VALIDATE_FLAG_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_FLAG_REASON_FAILED,
                    Self::VALIDATE_FLAG_MESSAGE_EMPTY,
                );
                false
            }
            Some(flag_str) if !validate_flag_rules(flag, flag_str) => {
                status.conditions.set(
                    Self::VALIDATE_FLAG_CONDITION_TYPE,
                    ConditionStatus::False,
                    Self::VALIDATE_FLAG_REASON_FAILED,
                    Self::VALIDATE_FLAG_MESSAGE_NOT_MATCHING,
                );
                false
            }
            _ => {
                status.conditions.set(
                    Self::VALIDATE_FLAG_CONDITION_TYPE,
                    ConditionStatus::True,
                    Self::VALIDATE_FLAG_REASON_SUCCEEDED,
                    "",
                );
                true
            }
        }
    }
}

fn has_duplicate_chars(s: &str) -> bool {
    let mut seen = HashSet::new();
    for c in s.chars() {
        if !seen.insert(c) {
            return true;
        }
    }
    false
}

fn validate_flag_rules(flag: &Flag, flag_str: &str) -> bool {
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
