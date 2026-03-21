use ctf_cli::{ChallengeCategory, ChallengeDifficulty};
use serde::{Deserialize, Serialize};

// ---- Public models ----

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub discord_id: Option<String>,
    pub discord_username: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagView {
    pub name: String,
    pub label: Option<String>,
    #[serde(default)]
    pub solved: bool,
    /// Base points when no one has solved (default: 500)
    #[serde(default = "default_base_points")]
    pub base_points: u32,
    /// Minimum points floor (default: 50)
    #[serde(default = "default_min_points")]
    pub min_points: u32,
    /// Decay factor (default: 6)
    #[serde(default = "default_decay")]
    pub decay: u32,
    /// Optional difficulty rating for this flag
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub difficulty: Option<ChallengeDifficulty>,
}

fn default_base_points() -> u32 {
    500
}
fn default_min_points() -> u32 {
    50
}
fn default_decay() -> u32 {
    6
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortView {
    pub name: String,
    pub port: u16,
    pub port_type: String,
    /// Allocated external port from PortClaim (if available)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allocated_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeView {
    pub namespace: String,
    pub name: String,
    pub ctf_name: String,
    pub ctf_public: bool,
    pub full_name: String,
    pub description: Option<String>,
    pub categories: Vec<ChallengeCategory>,
    pub files: Vec<String>,
    pub flags: Vec<FlagView>,
    pub exposed_ports: Vec<PortView>,
    /// Whether the challenge has any containers (and thus needs instances).
    pub has_containers: bool,
    /// Per-user instancing enabled for this challenge.
    pub instancing: bool,
    /// Flag format hint like "SCC{...}" for placeholder
    pub flag_hint: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SolvedKey {
    pub challenge_name: String,
    pub ctf_name: String,
}
