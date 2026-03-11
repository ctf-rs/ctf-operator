use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{ConditionStatus, Ctf, CtfStatus};
use kube::{runtime::controller::Action, ResourceExt};

pub struct CtfReconciler;

impl Reconciler for CtfReconciler {
    const NAME: &'static str = "ctf-controller";
    type K = Ctf;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Ctf>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = obj.namespace().unwrap_or_default();
        if !state.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = obj.status.clone().unwrap_or_default();
        validate_spec(&obj, &mut status);

        let ready = status.conditions.all_true();

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

fn validate_spec(resource: &Ctf, status: &mut CtfStatus) {
    let core = ctf_cli::models::Ctf {
        name: resource.name_any(),
        full_name: resource.spec.full_name.clone(),
        description: resource.spec.description.clone(),
        public: resource.spec.public,
        registry: None,
        storage: None,
        challenges: vec![],
        flag: resource.spec.flag.clone(),
        network: resource.spec.network.clone(),
    };

    match core.validate_strict() {
        Ok(()) => {
            status
                .conditions
                .set("Validation", ConditionStatus::True, "Validated", "");
        }
        Err(err) => {
            status.conditions.set(
                "Validation",
                ConditionStatus::False,
                "InvalidSpec",
                err.message(),
            );
        }
    }

    if status.conditions.all_true() {
        status
            .conditions
            .set("Ready", ConditionStatus::True, "Ready", "");
    } else {
        status
            .conditions
            .set("Ready", ConditionStatus::False, "NotReady", "");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctf_cli::k8s::CtfSpec;

    fn valid_ctf() -> Ctf {
        let mut ctf = Ctf::new(
            "sample-ctf",
            CtfSpec {
                full_name: "Sample CTF".to_string(),
                description: Some("desc".to_string()),
                conditions: Default::default(),
                challenges: vec!["chall-1".to_string()],
                flag: ctf_cli::models::CtfFlag {
                    prefix: "SCC".to_string(),
                    format: ctf_cli::models::FlagFormat::Curly,
                    random: None,
                },
                network: ctf_cli::models::CtfNetwork {
                    ports: ctf_cli::models::CtfPorts {
                        start: 20000,
                        end: 20100,
                    },
                    ip: None,
                },
            },
        );
        ctf.metadata.namespace = Some("ctf".to_string());
        ctf
    }

    #[test]
    fn validate_spec_sets_ready_true_for_valid_spec() {
        let ctf = valid_ctf();
        let mut status = CtfStatus::default();

        validate_spec(&ctf, &mut status);

        assert!(status.conditions.all_true());
    }

    #[test]
    fn validate_spec_sets_ready_false_for_invalid_spec() {
        let mut ctf = valid_ctf();
        ctf.spec.flag.prefix = "".to_string();

        let mut status = CtfStatus::default();
        validate_spec(&ctf, &mut status);

        assert!(!status.conditions.all_true());
        assert!(status.conditions.get("Validation").is_some());
        assert!(status.conditions.get("Ready").is_some());
    }
}
