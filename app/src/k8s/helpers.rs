use std::sync::Arc;

use ctf_cli::k8s::{Challenge, Ctf, Instance};
use kube::Api;

use crate::{
    db::User,
    helpers::now_unix_seconds,
    state::AppState,
};

/// Check whether an Instance resource has a `Ready=True` condition.
pub fn instance_ready_from_resource(instance: &Instance) -> bool {
    let Some(status) = instance.status.as_ref() else {
        return false;
    };

    let Ok(status_json) = serde_json::to_value(status) else {
        return false;
    };
    let Some(conditions) = status_json.get("conditions").and_then(|c| c.as_array()) else {
        return false;
    };

    conditions.iter().any(|cond| {
        cond.get("type").and_then(|v| v.as_str()) == Some("Ready")
            && cond.get("status").and_then(|v| v.as_str()) == Some("True")
    })
}

/// Seconds remaining before the instance expires, based on the
/// `app.ctf.rs/expires-at` annotation.
pub fn instance_time_left_seconds(instance: &Instance) -> Option<u64> {
    let now_unix = now_unix_seconds();
    instance
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get("app.ctf.rs/expires-at"))
        .and_then(|value| value.parse::<u64>().ok())
        .map(|expires_at| expires_at.saturating_sub(now_unix))
}

/// Does the instance belong to the given user (by annotation)?
pub fn is_instance_owned_by_user(instance: &Instance, user: &User) -> bool {
    instance
        .metadata
        .annotations
        .as_ref()
        .and_then(|annotations| annotations.get("app.ctf.rs/user-id"))
        .map(|value| value == &user.id.to_string())
        .unwrap_or(false)
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
