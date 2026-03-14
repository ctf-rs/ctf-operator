mod challenges;
mod flags;
mod instances;
mod models;
mod users;

pub use challenges::{
    find_challenge_namespace, get_challenge, list_challenges_for_ctf,
    replace_challenges_for_namespace,
};
pub use flags::{
    add_flag_submission, is_challenge_solved_for_user, list_solved_challenges_for_user,
    mark_challenge_solved,
};
pub use instances::{clear_instance_for_user, get_instance_for_user, set_instance};
#[allow(unused_imports)] // re-exported as public API
pub use models::{ChallengeView, InstanceView, SolvedKey, User};
pub use users::{create_session, delete_session, get_user_by_session, upsert_user};

use anyhow::Result;
use sqlx::{PgPool, postgres::PgPoolOptions};

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

    sqlx::query(
        "ALTER TABLE app_challenges ADD COLUMN IF NOT EXISTS ctf_public BOOLEAN NOT NULL DEFAULT false",
    )
    .execute(db)
    .await?;

    sqlx::query(
        "ALTER TABLE app_challenges ADD COLUMN IF NOT EXISTS categories_json JSONB NOT NULL DEFAULT '[]'::jsonb",
    )
    .execute(db)
    .await?;

    sqlx::query("ALTER TABLE app_challenges ADD COLUMN IF NOT EXISTS difficulty TEXT")
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

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS app_solved_challenges (
            user_id BIGINT NOT NULL REFERENCES app_users(id) ON DELETE CASCADE,
            challenge_namespace TEXT NOT NULL,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            solved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY(user_id, challenge_namespace, challenge_name, ctf_name)
        )"#,
    )
    .execute(db)
    .await?;

    Ok(())
}
