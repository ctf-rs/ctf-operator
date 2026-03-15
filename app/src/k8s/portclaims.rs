//! Kubernetes operations for PortClaim resources.

use ctf_cli::k8s::PortClaim;
use kube::{Api, Client, api::ListParams};

/// Lists all PortClaims in a namespace.
pub async fn list_port_claims(
    client: &Client,
    namespace: &str,
) -> Result<Vec<PortClaim>, kube::Error> {
    let api: Api<PortClaim> = Api::namespaced(client.clone(), namespace);
    let list = api.list(&ListParams::default()).await?;
    Ok(list.items)
}

/// Gets allocated ports for an instance from its PortClaims.
/// Returns a map of port_name -> allocated_port.
pub async fn get_allocated_ports_for_instance(
    client: &Client,
    namespace: &str,
    instance_name: &str,
) -> Result<std::collections::HashMap<String, u16>, kube::Error> {
    let claims = list_port_claims(client, namespace).await?;
    let mut ports = std::collections::HashMap::new();

    let prefix = format!("{}-", instance_name);
    for claim in claims {
        if let Some(status) = claim.status {
            if let Some(allocated) = status.port {
                if let Some(claim_name) = claim.metadata.name {
                    if let Some(port_name) = claim_name.strip_prefix(&prefix) {
                        ports.insert(port_name.to_string(), allocated);
                    }
                }
            }
        }
    }

    Ok(ports)
}
