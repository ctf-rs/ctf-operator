use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_k8s::{ConditionStatus, Flag, FlagStatus};
use kube::{runtime::controller::Action, ResourceExt};

pub struct FlagReconciler;

impl Reconciler for FlagReconciler {
    const NAME: &'static str = "flag-controller";
    type K = Flag;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<Flag>, state: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = obj.namespace().unwrap_or_default();
        if !state.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        let mut status = obj.status.clone().unwrap_or_default();
        validate_and_generate(&obj, &mut status);

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

fn validate_and_generate(resource: &Flag, status: &mut FlagStatus) {
    let core = ctf_core::Flag {
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
