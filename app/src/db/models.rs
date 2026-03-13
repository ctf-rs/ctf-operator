use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ---- Public models ----

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub sub: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub roles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChallengeView {
    pub namespace: String,
    pub name: String,
    pub ctf_name: String,
    pub ctf_public: bool,
    pub full_name: String,
    pub description: Option<String>,
    pub files: Vec<String>,
    pub flags: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct InstanceView {
    pub instance_name: String,
    pub namespace: String,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct SolvedKey {
    pub challenge_name: String,
    pub ctf_name: String,
}

// ---- Internal row types ----

#[derive(Debug, FromRow)]
pub(super) struct UserRow {
    pub(super) id: i64,
    pub(super) sub: String,
    pub(super) email: Option<String>,
    pub(super) name: Option<String>,
    pub(super) roles_json: sqlx::types::Json<Vec<String>>,
}

#[derive(Debug, FromRow)]
pub(super) struct ChallengeRow {
    pub(super) namespace: String,
    pub(super) name: String,
    pub(super) ctf_name: String,
    pub(super) ctf_public: bool,
    pub(super) full_name: String,
    pub(super) description: Option<String>,
    pub(super) files_json: sqlx::types::Json<Vec<String>>,
    pub(super) flags_json: sqlx::types::Json<Vec<String>>,
}

#[derive(Debug, FromRow)]
pub(super) struct InstanceRow {
    pub(super) instance_name: String,
    pub(super) challenge_namespace: String,
}

// ---- Row → model converters ----

pub(super) fn user_from_row(row: UserRow) -> User {
    User {
        id: row.id,
        sub: row.sub,
        email: row.email,
        name: row.name,
        roles: row.roles_json.0,
    }
}

pub(super) fn challenge_from_row(row: ChallengeRow) -> ChallengeView {
    ChallengeView {
        namespace: row.namespace,
        name: row.name,
        ctf_name: row.ctf_name,
        ctf_public: row.ctf_public,
        full_name: row.full_name,
        description: row.description,
        files: row.files_json.0,
        flags: row.flags_json.0,
    }
}
