use k8s_openapi::apimachinery::pkg::apis::meta::v1::Condition;
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    CustomResource, Serialize, Deserialize, Default, Debug, PartialEq, Eq, Clone, JsonSchema,
)]
#[kube(
    group = "ctf.rs",
    version = "v1",
    kind = "PortClaim",
    plural = "claims",
    root = "PortClaim",
    namespaced,
    doc = "Custom resource representing a PortClaim",
    status = "PortClaimStatus",
    derive = "PartialEq",
    derive = "Default",
    shortname = "pcl"
)]
pub struct PortClaimSpec {

}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PortClaimStatus {
    pub claim: String,
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<Condition>,
}


#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, JsonSchema)]
pub struct PortClaimRef
{
    pub name: String,
    pub namespace: String,
}
