//! Kubernetes operations for Instance resources.

use ctf_cli::k8s::Instance;
use kube::{
    Api, Client,
    api::{DeleteParams, Patch, PatchParams},
};

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
