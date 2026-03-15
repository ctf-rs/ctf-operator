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

/// Extends the TTL of an Instance by updating its annotations.
pub async fn extend_instance_ttl(
    client: &Client,
    namespace: &str,
    name: &str,
    duration_seconds: u64,
    new_expires_at: u64,
) -> Result<(), kube::Error> {
    let api: Api<Instance> = Api::namespaced(client.clone(), namespace);

    let patch = serde_json::json!({
        "metadata": {
            "annotations": {
                "app.ctf.rs/duration-seconds": duration_seconds.to_string(),
                "app.ctf.rs/expires-at": new_expires_at.to_string(),
            }
        }
    });

    api.patch(name, &PatchParams::apply("ctf-app"), &Patch::Merge(&patch))
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
    let user_id_str = user_id.to_string();
    state.instances.state().into_iter().find(|instance| {
        let annotations = instance.metadata.annotations.as_ref();
        let matches_user = annotations
            .and_then(|a| a.get("app.ctf.rs/user-id"))
            .map(|v| v == &user_id_str)
            .unwrap_or(false);
        matches_user && instance.spec.challenge == challenge_name && instance.spec.ctf == ctf_name
    })
}

/// List all active instances owned by a user (from Store).
pub fn list_instances_for_user(state: &AppState, user_id: i64) -> Vec<Arc<Instance>> {
    let user_id_str = user_id.to_string();
    state
        .instances
        .state()
        .into_iter()
        .filter(|instance| {
            instance
                .metadata
                .annotations
                .as_ref()
                .and_then(|a| a.get("app.ctf.rs/user-id"))
                .map(|v| v == &user_id_str)
                .unwrap_or(false)
        })
        .collect()
}
