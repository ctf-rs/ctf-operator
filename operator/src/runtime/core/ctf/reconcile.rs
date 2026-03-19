use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState, MANAGED_BY_LABEL, MANAGED_BY_VALUE},
    util::Reconciler,
};
use ctf_cli::k8s::{ConditionStatus, Ctf, CtfStatus, ReservationSet, ReservationSetSpec};
use kube::{
    api::{ObjectMeta, Patch, PatchParams},
    runtime::controller::Action,
    Api, ResourceExt,
};

#[derive(Debug)]
pub struct CtfReconciler;

impl Reconciler for CtfReconciler {
    const NAME: &'static str = "ctf-controller";
    type K = Ctf;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(resource: Arc<Ctf>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = resource.namespace().unwrap_or_default();
        if !ctx.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = resource.status.clone().unwrap_or_default();
        validate_spec(&resource, &mut status);

        let ready = status.conditions.all_true();

        // Create / update the per-CTF ReservationSet (shared by all instances).
        if ready {
            ensure_reservation_set(&resource, &namespace, &ctx).await?;
        }

        if resource.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(resource.as_ref(), ctx.client.clone(), status).await?;
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

/// Creates or updates the single `ReservationSet` that all instances under
/// this CTF share for port allocation.
async fn ensure_reservation_set(
    ctf: &Ctf,
    namespace: &str,
    state: &CtfState,
) -> Result<(), CtfError> {
    let name = ctf.name_any();
    let labels = std::collections::BTreeMap::from([
        (MANAGED_BY_LABEL.to_string(), MANAGED_BY_VALUE.to_string()),
        ("app.kubernetes.io/name".to_string(), name.clone()),
    ]);

    let resource = ReservationSet {
        metadata: ObjectMeta {
            name: Some(name.clone()),
            namespace: Some(namespace.to_owned()),
            labels: Some(labels),
            ..Default::default()
        },
        spec: ReservationSetSpec {
            start: ctf.spec.network.ports.start,
            end: ctf.spec.network.ports.end,
        },
        status: None,
    };

    Api::<ReservationSet>::namespaced(state.client.clone(), namespace)
        .patch(
            &name,
            &PatchParams::apply(CtfReconciler::NAME).force(),
            &Patch::Apply(&resource),
        )
        .await?;

    Ok(())
}

fn validate_spec(resource: &Ctf, status: &mut CtfStatus) {
    let core = ctf_cli::models::Ctf {
        name: resource.name_any(),
        full_name: resource.spec.full_name.clone(),
        description: resource.spec.description.clone(),
        public: resource.spec.public,
        instance_duration_seconds: resource.spec.instance_duration_seconds,
        registry: None,
        storage: None,
        challenges: vec![],
        flag: resource.spec.flag.clone(),
        network: resource.spec.network.clone(),
        scoreboard: resource.spec.scoreboard.clone(),
        schedule: resource.spec.schedule.clone(),
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
                public: false,
                instance_duration_seconds: 15 * 60,
                challenges: vec!["chall-1".to_string()],
                flag: ctf_cli::models::CtfFlag {
                    prefix: "SCC".to_string(),
                    format: ctf_cli::models::FlagFormat::Curly,
                    random: None,
                    options: ctf_cli::models::FlagMatchOptions::default(),
                },
                network: ctf_cli::models::CtfNetwork {
                    ports: ctf_cli::models::CtfPorts {
                        start: 20000,
                        end: 20100,
                    },
                    ip: None,
                },
                scoreboard: ctf_cli::models::CtfScoreboard::default(),
                schedule: ctf_cli::models::CtfSchedule::default(),
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
        ctf.spec.flag.prefix = String::new();

        let mut status = CtfStatus::default();
        validate_spec(&ctf, &mut status);

        assert!(!status.conditions.all_true());
        assert!(status.conditions.get("Validation").is_some());
        assert!(status.conditions.get("Ready").is_some());
    }
}
