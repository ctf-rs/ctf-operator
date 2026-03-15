mod audit;
mod flags;
pub mod models;
mod users;

pub use audit::{check_flag_cheated, log_download, log_flag_generation, log_instance_action};
pub use flags::{
    add_flag_submission, count_solved_flags_for_user, get_partial_solve_counts_for_user,
    get_solved_flags_for_user, is_challenge_solved_for_user, list_solved_challenges_for_user,
    mark_challenge_solved,
};
#[allow(unused_imports)] // re-exported as public API
pub use models::{ChallengeView, FlagView, PortView, SolvedKey, User};
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
        r#"CREATE TABLE IF NOT EXISTS users (
            id BIGSERIAL PRIMARY KEY,
            sub TEXT NOT NULL UNIQUE,
            email TEXT,
            name TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            roles_json JSONB NOT NULL DEFAULT '[]'::jsonb,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    // Tracks per-instance generated flags (for cheat detection)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS instance_flags (
            id BIGSERIAL PRIMARY KEY,
            instance_name TEXT NOT NULL,
            user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            flag_name TEXT NOT NULL,
            generated_flag TEXT NOT NULL,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    // Index for efficient cheat detection lookup
    sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_instance_flags_lookup
           ON instance_flags(generated_flag, ctf_name)"#,
    )
    .execute(db)
    .await?;

    // All flag submissions (merged with cheat detection)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS flag_submissions (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            flag_name TEXT NOT NULL,
            submitted_flag TEXT NOT NULL,
            accepted BOOLEAN NOT NULL,
            cheated BOOLEAN NOT NULL DEFAULT false,
            cheated_from_user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
            cheated_flag_gen_id BIGINT REFERENCES instance_flags(id) ON DELETE SET NULL,
            ip_address TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS solved_challenges (
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            solved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY(user_id, challenge_name, ctf_name)
        )"#,
    )
    .execute(db)
    .await?;

    // Instance actions (spawn/stop/extend)
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS instance_actions (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            instance_name TEXT NOT NULL,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            action TEXT NOT NULL,
            ip_address TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    // File downloads
    sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS downloads (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            file_url TEXT NOT NULL,
            ip_address TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .execute(db)
    .await?;

    Ok(())
}
