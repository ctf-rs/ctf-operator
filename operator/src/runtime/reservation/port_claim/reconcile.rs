use crate::crd::v1alpha1::PortClaimStatus;
use crate::util::{ConditionStatus, KubeObject};
use crate::{
    crd::v1alpha1::PortClaim,
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use kube::ResourceExt;
use kube::runtime::controller::Action;
use std::sync::Arc;
use tracing::info;

pub struct PortClaimReconciler {}
impl Reconciler for PortClaimReconciler {
    const NAME: &'static str = "port-claim-controller";
    type K = PortClaim;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(obj: Arc<PortClaim>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        info!(
            "Reconciling {} {} {}",
            Self::K::gvk(),
            obj.name_unchecked(),
            obj.namespace_unchecked()
        );

        let port_claim = obj.as_ref();
        let mut status = obj.as_ref().status.to_owned().unwrap_or_default();

        if Self::validate_spec(port_claim, &mut status) {
        } else {
            status.port = None;
        }

        if port_claim.status.as_ref() != Some(&status) {
            Self::patch_status_namespaced(port_claim, ctx.client.clone(), status).await?;
        }

        Ok(Action::await_change())
    }
}

impl PortClaimReconciler {
    const VALIDATE_SPEC_CONDITION_TYPE: &'static str = "ValidateSpec";
    const VALIDATE_SPEC_REASON_SUCCEEDED: &'static str = "Validated";

    fn validate_spec(_port_claim: &PortClaim, status: &mut PortClaimStatus) -> bool {
        status.conditions.set(
            Self::VALIDATE_SPEC_CONDITION_TYPE,
            ConditionStatus::True,
            Self::VALIDATE_SPEC_REASON_SUCCEEDED,
            "",
        );
        true
    }
}
