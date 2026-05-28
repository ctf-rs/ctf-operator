use std::sync::Arc;

use ctf_cli::k8s::{Challenge, Ctf, Instance, InstancePhase, InstanceTimeLeft};
use kube::Api;

use crate::{db::User, state::AppState};

/// Check whether an Instance resource has a `Ready=True` condition.
pub fn instance_ready_from_resource(instance: &Instance) -> bool {
    let Some(status) = instance.status.as_ref() else {
        return false;
    };
    status.phase == InstancePhase::Ready
}

/// Check whether an Instance is being deleted (has deletion_timestamp set).
pub fn is_instance_deleting(instance: &Instance) -> bool {
    instance.metadata.deletion_timestamp.is_some()
}

/// Get the current phase of the instance.
pub fn instance_phase_from_resource(instance: &Instance) -> InstancePhase {
    instance
        .status
        .as_ref()
        .map(|s| s.phase)
        .unwrap_or(InstancePhase::Initializing)
}

/// Get the phase description (human-readable) from an instance.
pub fn instance_phase_description(instance: &Instance) -> String {
    let phase = instance_phase_from_resource(instance);
    let base_desc = phase.description().to_string();

    // If there's a custom message (e.g., for errors), append it
    if let Some(msg) = instance.status.as_ref().and_then(|s| s.message.as_ref()) {
        if !msg.is_empty() {
            return format!("{} {}", base_desc, msg);
        }
    }
    base_desc
}

/// Seconds remaining before the instance expires, based on
/// `status.expires_at`.
pub fn instance_time_left_seconds(instance: &Instance) -> Option<u64> {
    match instance.status.as_ref()?.time_left()? {
        InstanceTimeLeft::Remaining(dur) => Some(dur.as_secs()),
        InstanceTimeLeft::Expired => Some(0),
    }
}

/// Unix epoch (seconds) when the instance timer expires.
/// Returns `None` when `expires_at` is not set (timer not started).
pub fn instance_expires_at_epoch(instance: &Instance) -> Option<u64> {
    let expires_at = instance.status.as_ref()?.expires_at.as_ref()?;
    u64::try_from(expires_at.0.as_second()).ok()
}

/// Does the instance belong to the given user or their team?
/// - Team instance: any teammate is considered the owner.
/// - User instance (no team): only the exact user.
/// - Global instances (no user, no team): accessible by everyone.
pub fn is_instance_owned_by_user(instance: &Instance, user: &User) -> bool {
    // Team-owned instance: check team_id match
    if let Some(team) = &instance.spec.team {
        return user.team_id.as_ref().is_some_and(|tid| tid == &team.id);
    }
    // User-owned or global instance
    match &instance.spec.user {
        Some(u) => u.id == user.id,
        None => true, // global instance — shared by all users
    }
}

/// Resolve the instance lifetime in seconds by checking the Challenge first,
/// then falling back to the CTF default.
pub async fn resolve_instance_duration_seconds(
    state: &Arc<AppState>,
    namespace: &str,
    ctf_name: &str,
    challenge_name: &str,
) -> u64 {
    let ctf_api: Api<Ctf> = Api::namespaced(state.kube.clone(), namespace);
    let challenge_api: Api<Challenge> = Api::namespaced(state.kube.clone(), namespace);

    let ctf_duration = ctf_api
        .get(ctf_name)
        .await
        .ok()
        .map(|ctf| u64::from(ctf.spec.instance_duration_seconds))
        .unwrap_or(15 * 60);

    challenge_api
        .get(challenge_name)
        .await
        .ok()
        .and_then(|challenge| challenge.spec.instance_duration_seconds.map(u64::from))
        .unwrap_or(ctf_duration)
}
