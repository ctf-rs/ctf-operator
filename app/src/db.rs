use anyhow::Result;
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, FromRow, PgPool};

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

#[derive(Debug, FromRow)]
struct UserRow {
    id: i64,
    sub: String,
    email: Option<String>,
    name: Option<String>,
    roles_json: sqlx::types::Json<Vec<String>>,
}

#[derive(Debug, FromRow)]
struct ChallengeRow {
    namespace: String,
    name: String,
    ctf_name: String,
    ctf_public: bool,
    full_name: String,
    description: Option<String>,
    files_json: sqlx::types::Json<Vec<String>>,
    flags_json: sqlx::types::Json<Vec<String>>,
}

#[derive(Debug, FromRow)]
struct InstanceRow {
    instance_name: String,
    challenge_namespace: String,
}

pub async fn connect(database_url: &str) -> Result<PgPool> {
    Ok(PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await?)
}

pub async fn migrate(db: &PgPool) -> Result<()> {
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS app_users (
            id BIGSERIAL PRIMARY KEY,
            sub TEXT NOT NULL UNIQUE,
            email TEXT,
            name TEXT,
            roles_json JSONB NOT NULL DEFAULT '[]'::jsonb,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS app_sessions (
            token TEXT PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS app_challenges (
            namespace TEXT NOT NULL,
            name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            ctf_public BOOLEAN NOT NULL DEFAULT false,
            full_name TEXT NOT NULL,
            description TEXT,
            files_json JSONB NOT NULL DEFAULT '[]'::jsonb,
            flags_json JSONB NOT NULL DEFAULT '[]'::jsonb,
            updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY(namespace, name, ctf_name)
        )"#,
    )
    .execute(db)
    .await?;

    sqlx::query("ALTER TABLE app_challenges ADD COLUMN IF NOT EXISTS ctf_public BOOLEAN NOT NULL DEFAULT false")
        .execute(db)
        .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS app_instances (
            user_id BIGINT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
            challenge_namespace TEXT NOT NULL,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            instance_name TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY(user_id, challenge_namespace, challenge_name, ctf_name)
        )"#,
    )
    .execute(db)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS app_flag_submissions (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
            challenge_namespace TEXT NOT NULL,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            flag_name TEXT NOT NULL,
            submitted_flag TEXT NOT NULL,
            accepted BOOLEAN NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    Ok(())
}

pub async fn upsert_user(
    db: &PgPool,
    sub: &str,
    email: Option<&str>,
    name: Option<&str>,
    roles: &[String],
) -> Result<User> {
    let row = sqlx::query_as::<_, UserRow>(
        r#"INSERT INTO app_users(sub, email, name, roles_json)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT(sub) DO UPDATE
             SET email = EXCLUDED.email,
                 name = EXCLUDED.name,
                 roles_json = EXCLUDED.roles_json
           RETURNING id, sub, email, name, roles_json"#,
    )
    .bind(sub)
    .bind(email)
    .bind(name)
    .bind(sqlx::types::Json(roles.to_vec()))
    .fetch_one(db)
    .await?;

    Ok(user_from_row(row))
}

pub async fn create_session(db: &PgPool, token: &str, user_id: i64) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_sessions(token, user_id)
           VALUES ($1, $2)
           ON CONFLICT(token) DO UPDATE SET user_id = EXCLUDED.user_id"#,
    )
    .bind(token)
    .bind(user_id)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn delete_session(db: &PgPool, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM app_sessions WHERE token = $1")
        .bind(token)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_user_by_session(db: &PgPool, token: &str) -> Result<Option<User>> {
    let row = sqlx::query_as::<_, UserRow>(
        r#"SELECT u.id, u.sub, u.email, u.name, u.roles_json
           FROM app_users u
           INNER JOIN app_sessions s ON s.user_id = u.id
           WHERE s.token = $1"#,
    )
    .bind(token)
    .fetch_optional(db)
    .await?;

    Ok(row.map(user_from_row))
}

pub async fn replace_challenges_for_namespace(
    db: &PgPool,
    namespace: &str,
    rows: &[ChallengeView],
) -> Result<()> {
    sqlx::query("DELETE FROM app_challenges WHERE namespace = $1")
        .bind(namespace)
        .execute(db)
        .await?;

    for row in rows {
        sqlx::query(
            r#"INSERT INTO app_challenges(
                namespace, name, ctf_name, ctf_public, full_name, description, files_json, flags_json
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"#,
        )
        .bind(&row.namespace)
        .bind(&row.name)
        .bind(&row.ctf_name)
        .bind(row.ctf_public)
        .bind(&row.full_name)
        .bind(&row.description)
        .bind(sqlx::types::Json(row.files.clone()))
        .bind(sqlx::types::Json(row.flags.clone()))
        .execute(db)
        .await?;
    }

    Ok(())
}

pub async fn list_challenges(db: &PgPool) -> Result<Vec<ChallengeView>> {
    let rows = sqlx::query_as::<_, ChallengeRow>(
        r#"SELECT namespace, name, ctf_name, ctf_public, full_name, description, files_json, flags_json
           FROM app_challenges ORDER BY full_name ASC"#,
    )
    .fetch_all(db)
    .await?;

    Ok(rows.into_iter().map(challenge_from_row).collect())
}

pub async fn get_challenge(
    db: &PgPool,
    ctf_name: &str,
    challenge_name: &str,
) -> Result<Option<ChallengeView>> {
    let row = sqlx::query_as::<_, ChallengeRow>(
        r#"SELECT namespace, name, ctf_name, ctf_public, full_name, description, files_json, flags_json
           FROM app_challenges
           WHERE ctf_name = $1 AND name = $2
           ORDER BY updated_at DESC
           LIMIT 1"#,
    )
    .bind(ctf_name)
    .bind(challenge_name)
    .fetch_optional(db)
    .await?;

    Ok(row.map(challenge_from_row))
}

pub async fn set_instance(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    challenge_namespace: &str,
    instance_name: &str,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_instances(user_id, challenge_namespace, challenge_name, ctf_name, instance_name)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT(user_id, challenge_namespace, challenge_name, ctf_name) DO UPDATE
             SET instance_name = EXCLUDED.instance_name,
                 created_at = now()"#,
    )
    .bind(user_id)
    .bind(challenge_namespace)
    .bind(challenge_name)
    .bind(ctf_name)
    .bind(instance_name)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn get_instance_for_user(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<Option<InstanceView>> {
    let row = sqlx::query_as::<_, InstanceRow>(
        r#"SELECT instance_name, challenge_namespace
           FROM app_instances
           WHERE user_id = $1 AND challenge_name = $2 AND ctf_name = $3
           ORDER BY created_at DESC
           LIMIT 1"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|r| InstanceView {
        instance_name: r.instance_name,
        namespace: r.challenge_namespace,
    }))
}

pub async fn add_flag_submission(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    challenge_namespace: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_flag_submissions(
            user_id, challenge_namespace, challenge_name, ctf_name, flag_name, submitted_flag, accepted
        ) VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    )
    .bind(user_id)
    .bind(challenge_namespace)
    .bind(challenge_name)
    .bind(ctf_name)
    .bind(flag_name)
    .bind(submitted_flag)
    .bind(accepted)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn find_challenge_namespace(
    db: &PgPool,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<Option<String>> {
    let row = sqlx::query_scalar::<_, String>(
        r#"SELECT namespace
           FROM app_challenges
           WHERE name = $1 AND ctf_name = $2
           ORDER BY updated_at DESC
           LIMIT 1"#,
    )
    .bind(challenge_name)
    .bind(ctf_name)
    .fetch_optional(db)
    .await?;

    Ok(row)
}

fn user_from_row(row: UserRow) -> User {
    User {
        id: row.id,
        sub: row.sub,
        email: row.email,
        name: row.name,
        roles: row.roles_json.0,
    }
}

fn challenge_from_row(row: ChallengeRow) -> ChallengeView {
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
