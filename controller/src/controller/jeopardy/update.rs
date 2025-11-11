use kube::api::Patch;

#[derive(Debug, PartialEq)]
pub struct Update {
    pub kind: UpdateKind,
    pub patch: Patch<serde_json::Value>,
}

#[derive(Debug, PartialEq)]
pub enum UpdateKind {
    Ctf(String, String),
    Challenge(String, String),
    Instance(String, String),
}
