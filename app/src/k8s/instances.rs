//! Kubernetes operations for Instance resources.

use std::sync::Arc;

use ctf_cli::k8s::Instance;
use kube::{
    Api, Client,
    api::{DeleteParams, Patch, PatchParams},
};

use crate::state::AppState;

/// Creates or updates an Instance resource.
pub async fn create_or_update_instance(
    client: &Client,
    namespace: &str,
    instance: &Instance,
) -> Result<Instance, kube::Error> {
    let api: Api<Instance> = Api::namespaced(client.clone(), namespace);
    api.patch(
        &instance.metadata.name.clone().unwrap_or_default(),
        &PatchParams::apply("ctf-app").force(),
        &Patch::Apply(instance),
    )
    .await
}

/// Deletes an Instance resource.
pub async fn delete_instance(
    client: &Client,
    namespace: &str,
    name: &str,
) -> Result<(), kube::Error> {
    let api: Api<Instance> = Api::namespaced(client.clone(), namespace);
    match api.delete(name, &DeleteParams::default()).await {
        Ok(_) => Ok(()),
        Err(kube::Error::Api(ae)) if ae.code == 404 => Ok(()), // Already gone
        Err(err) => Err(err),
    }
}

/// Extends an instance's lifetime by patching `spec.durationSeconds`.
///
/// The operator will pick up the new value and recompute `status.expiresAt`.
pub async fn extend_instance_duration(
    client: &Client,
    namespace: &str,
    name: &str,
    new_duration_seconds: u64,
) -> Result<(), kube::Error> {
    let api: Api<Instance> = Api::namespaced(client.clone(), namespace);

    let patch = serde_json::json!({
        "spec": {
            "durationSeconds": new_duration_seconds,
        }
    });

    api.patch(name, &PatchParams::default(), &Patch::Merge(&patch))
        .await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Store-based lookups (replaces DB app_instances table)
// ---------------------------------------------------------------------------

/// Find the user's active instance for a specific challenge (from Store).
pub fn get_instance_for_user(
    state: &AppState,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Option<Arc<Instance>> {
    state.instances.state().into_iter().find(|instance| {
        let matches_user = instance
            .spec
            .user
            .as_ref()
            .map(|u| u.id == user_id)
            .unwrap_or(false);
        matches_user && instance.spec.challenge == challenge_name && instance.spec.ctf == ctf_name
    })
}

/// Find the global (shared) instance for a challenge — no specific user.
pub fn get_global_instance(
    state: &AppState,
    challenge_name: &str,
    ctf_name: &str,
) -> Option<Arc<Instance>> {
    state.instances.state().into_iter().find(|instance| {
        instance.spec.user.is_none()
            && instance.spec.challenge == challenge_name
            && instance.spec.ctf == ctf_name
    })
}

/// Find the active instance for a user+challenge, falling back to a global instance.
pub fn get_instance_for_challenge(
    state: &AppState,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Option<Arc<Instance>> {
    get_instance_for_user(state, user_id, challenge_name, ctf_name)
        .or_else(|| get_global_instance(state, challenge_name, ctf_name))
}

/// List all active instances owned by a user (from Store).
pub fn list_instances_for_user(state: &AppState, user_id: i64) -> Vec<Arc<Instance>> {
    state
        .instances
        .state()
        .into_iter()
        .filter(|instance| {
            instance
                .spec
                .user
                .as_ref()
                .map(|u| u.id == user_id)
                .unwrap_or(false)
        })
        .collect()
}
