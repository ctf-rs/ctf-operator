use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_k8s::{ConditionStatus, Ctf, CtfStatus};
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
    let core = ctf_core::Ctf {
        name: resource.name_any(),
        full_name: resource.spec.full_name.clone(),
        description: resource.spec.description.clone(),
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
