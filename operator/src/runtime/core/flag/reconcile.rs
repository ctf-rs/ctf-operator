use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{ConditionStatus, Flag, FlagStatus};
use kube::{runtime::controller::Action, ResourceExt};

pub struct FlagReconciler;

impl Reconciler for FlagReconciler {
    const NAME: &'static str = "flag-controller";
    type K = Flag;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Flag>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = obj.namespace().unwrap_or_default();
        if !state.manages_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = obj.status.clone().unwrap_or_default();
        validate_and_generate(&obj, &mut status);

        let ready = conditions_true_excluding_ready(&status.conditions);

        if obj.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(obj.as_ref(), state.client.clone(), status).await?;
        }

        if ready {
            Ok(Action::await_change())
        } else {
            Ok(Action::requeue(Duration::from_secs(
                Self::ERROR_RECONCILE_REQUEUE_TIME,
            )))
        }
    }
}

fn validate_and_generate(resource: &Flag, status: &mut FlagStatus) {
    let core = ctf_cli::models::Flag {
        prefix: resource.spec.prefix.clone(),
        format: resource.spec.format,
        body: resource.spec.body.clone(),
        random: resource.spec.random.clone(),
        options: resource.spec.options.clone(),
    };

    match core.validate_strict() {
        Ok(()) => {
            status
                .conditions
                .set("Validation", ConditionStatus::True, "Validated", "");
            if status.flag.is_none() {
                status.flag = Some(core.generate());
            }
            status
                .conditions
                .set("Generate", ConditionStatus::True, "Generated", "");
        }
        Err(err) => {
            status.flag = None;
            status.conditions.set(
                "Validation",
                ConditionStatus::False,
                "InvalidSpec",
                err.message(),
            );
            status.conditions.set(
                "Generate",
                ConditionStatus::False,
                "Skipped",
                "Invalid flag spec",
            );
        }
    }

    if conditions_true_excluding_ready(&status.conditions) {
        status
            .conditions
            .set("Ready", ConditionStatus::True, "Ready", "");
    } else {
        status
            .conditions
            .set("Ready", ConditionStatus::False, "NotReady", "");
    }
}

fn conditions_true_excluding_ready(conditions: &ctf_cli::k8s::Conditions) -> bool {
    let mut clone = conditions.clone();
    clone.remove("Ready");
    clone.all_true()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::FlagSpec;

    fn valid_flag() -> Flag {
        let mut flag = Flag::new(
            "flag-1",
            FlagSpec {
                prefix: "SCC".to_string(),
                format: ctf_cli::models::FlagFormat::Curly,
                body: "hello".to_string(),
                random: None,
                options: Default::default(),
                conditions: Default::default(),
            },
        );
        flag.metadata.namespace = Some("ctf".to_string());
        flag
    }

    #[test]
    fn validate_and_generate_sets_ready_and_generated_flag() {
        let flag = valid_flag();
        let mut status = FlagStatus::default();

        validate_and_generate(&flag, &mut status);

        assert!(status.conditions.all_true());
        let generated = status.flag.as_ref().expect("expected generated flag");
        assert!(generated.starts_with("SCC{"));
        assert!(generated.ends_with('}'));
    }

    #[test]
    fn validate_and_generate_keeps_existing_flag_when_valid() {
        let flag = valid_flag();
        let mut status = FlagStatus {
            flag: Some("preexisting".to_string()),
            conditions: Default::default(),
        };

        validate_and_generate(&flag, &mut status);

        assert_eq!(status.flag.as_deref(), Some("preexisting"));
        assert!(status.conditions.all_true());
    }

    #[test]
    fn validate_and_generate_sets_not_ready_for_invalid_spec() {
        let mut flag = valid_flag();
        flag.spec.prefix = "".to_string();

        let mut status = FlagStatus {
            flag: Some("will-be-cleared".to_string()),
            conditions: Default::default(),
        };

        validate_and_generate(&flag, &mut status);

        assert_eq!(status.flag, None);
        assert!(!status.conditions.all_true());
        assert!(status.conditions.get("Validation").is_some());
        assert!(status.conditions.get("Generate").is_some());
        assert!(status.conditions.get("Ready").is_some());
    }
}
