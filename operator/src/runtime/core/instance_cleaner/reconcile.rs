use std::{sync::Arc, time::Duration};

use crate::{
    runtime::{CtfError, CtfState},
    util::Reconciler,
};
use ctf_cli::k8s::{Instance, InstancePhase};
use kube::{api::DeleteParams, runtime::controller::Action, Api, ResourceExt};

/// How often every instance is re-checked for expiry.
const REQUEUE_INTERVAL: Duration = Duration::from_secs(15);

#[derive(Debug)]
pub struct InstanceCleanerReconciler;

impl Reconciler for InstanceCleanerReconciler {
    const NAME: &'static str = "instance-cleaner-controller";
    type K = Instance;
    type Ctx = CtfState;
    type Err = CtfError;

    async fn reconcile(resource: Arc<Instance>, ctx: Arc<CtfState>) -> Result<Action, CtfError> {
        let namespace = resource.namespace().unwrap_or_default();
        if !ctx.watches_namespace(&namespace) {
            return Ok(Action::await_change());
        }

        // Nothing to do while the instance is being deleted.
        if resource.metadata.deletion_timestamp.is_some() {
            return Ok(Action::await_change());
        }

        // Timer not started yet — nothing to clean up.
        let Some(ready_since) = resource
            .status
            .as_ref()
            .and_then(|s| s.ready_since.as_ref())
        else {
            return Ok(Action::requeue(REQUEUE_INTERVAL));
        };

        // Already terminating — skip.
        if resource
            .status
            .as_ref()
            .is_some_and(|s| s.phase == InstancePhase::Terminating)
        {
            return Ok(Action::await_change());
        }

        let ready_since_unix = u64::try_from(ready_since.0.as_second()).unwrap_or(0);

        // Resolve effective duration: spec override → default from CTF/Challenge.
        // If the instance has no duration (global / always-on), never expire it.
        let Some(duration) = resolve_effective_duration(&resource, &ctx) else {
            return Ok(Action::await_change());
        };
        let expires_at = ready_since_unix.saturating_add(duration);
        let now_unix = now_unix_seconds();

        if now_unix >= expires_at {
            let api: Api<Instance> = Api::namespaced(ctx.client.clone(), &namespace);
            match api
                .delete(&resource.name_any(), &DeleteParams::default())
                .await
            {
                Ok(_) => {}
                Err(kube::Error::Api(ref ae)) if ae.code == 404 => {}
                Err(err) => return Err(err.into()),
            }
            return Ok(Action::await_change());
        }

        // Re-check just after the expected expiry (with a small margin).
        let remaining = expires_at.saturating_sub(now_unix);
        let requeue_secs = remaining.min(REQUEUE_INTERVAL.as_secs());
        Ok(Action::requeue(Duration::from_secs(requeue_secs)))
    }
}

/// Resolves the effective lifetime for this instance by looking at
/// `spec.duration_seconds`, then the Challenge default, then the CTF default.
/// Returns `None` when the instance has no duration (global / always-on).
fn resolve_effective_duration(instance: &Instance, state: &CtfState) -> Option<u64> {
    use kube::runtime::reflector::ObjectRef;

    // An explicit duration_seconds on the spec always wins.
    // If the spec field is present, use it; if the spec field is absent
    // (None) that means "no duration" — the instance should live forever.
    if instance.spec.duration_seconds.is_some() {
        return instance.spec.duration_seconds;
    }

    // No user field means this is a global / always-on instance — no expiry.
    if instance.spec.user.is_none() {
        return None;
    }

    let namespace = instance.namespace().unwrap_or_default();

    // Try challenge-level override.
    let challenge_ref =
        ObjectRef::<ctf_cli::k8s::Challenge>::new(&instance.spec.challenge).within(&namespace);
    if let Some(challenge) = state.challenges.get(&challenge_ref) {
        if let Some(d) = challenge.spec.instance_duration_seconds {
            return Some(u64::from(d));
        }
    }

    // Fall back to CTF-level default.
    let ctf_ref = ObjectRef::<ctf_cli::k8s::Ctf>::new(&instance.spec.ctf).within(&namespace);
    if let Some(ctf) = state.ctfs.get(&ctf_ref) {
        return Some(u64::from(ctf.spec.instance_duration_seconds));
    }

    // Absolute fallback (15 minutes).
    Some(15 * 60)
}

fn now_unix_seconds() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
