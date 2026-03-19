mod audit;
pub mod entity;
mod flags;
pub mod models;
mod users;

pub use audit::{
    FlagOrigin, check_flag_origin, log_download, log_flag_generation, log_instance_action,
};
pub use flags::{
    add_flag_submission, count_solved_flags_for_user, get_partial_solve_counts_for_user,
    get_solved_flags_for_user, is_challenge_solved_for_user, list_solved_challenges_for_user,
    mark_challenge_solved,
};
#[allow(unused_imports)] // re-exported as public API
pub use models::{ChallengeView, FlagView, PortView, SolvedKey, User};
pub use users::{create_session, delete_session, get_user_by_session, link_discord, upsert_user};

use anyhow::Result;
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};

pub async fn connect(database_url: &str) -> Result<DatabaseConnection> {
    let mut opt = ConnectOptions::new(database_url);
    opt.max_connections(10);
    Ok(Database::connect(opt).await?)
}

pub async fn migrate(db: &DatabaseConnection) -> Result<()> {
    db.execute_unprepared(
        r#"CREATE TABLE IF NOT EXISTS users (
            id BIGSERIAL PRIMARY KEY,
            sub TEXT NOT NULL UNIQUE,
            email TEXT,
            name TEXT,
            discord_id TEXT,
            discord_username TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .await?;

    // Add discord columns if they don't exist (migration for existing databases)
    db.execute_unprepared(
        r#"DO $$ BEGIN
            ALTER TABLE users ADD COLUMN IF NOT EXISTS discord_id TEXT;
            ALTER TABLE users ADD COLUMN IF NOT EXISTS discord_username TEXT;
        END $$"#,
    )
    .await?;

    db.execute_unprepared(
        r#"CREATE TABLE IF NOT EXISTS sessions (
            token TEXT PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            roles_json JSONB NOT NULL DEFAULT '[]'::jsonb,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .await?;

    // Tracks per-instance generated flags (for cheat detection)
    db.execute_unprepared(
        r#"CREATE TABLE IF NOT EXISTS instance_flags (
            id BIGSERIAL PRIMARY KEY,
            instance_name TEXT NOT NULL,
            user_id BIGINT REFERENCES users(id) ON DELETE SET NULL,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            flag_name TEXT NOT NULL,
            generated_flag TEXT NOT NULL,
            ignore_case BOOLEAN NOT NULL DEFAULT false,
            ignore_random BOOLEAN NOT NULL DEFAULT false,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .await?;

    // Index for efficient cheat detection lookup
    db.execute_unprepared(
        r#"CREATE INDEX IF NOT EXISTS idx_instance_flags_lookup
           ON instance_flags(generated_flag, ctf_name)"#,
    )
    .await?;

    // All flag submissions (merged with cheat detection)
    db.execute_unprepared(
        r#"CREATE TABLE IF NOT EXISTS flag_submissions (
            id BIGSERIAL PRIMARY KEY,
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            flag_name TEXT NOT NULL,
            submitted_flag TEXT NOT NULL,
            accepted BOOLEAN NOT NULL,
            cheated BOOLEAN NOT NULL DEFAULT false,
            cheated_instance_flag_id BIGINT REFERENCES instance_flags(id) ON DELETE SET NULL,
            ip_address TEXT,
            created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        )"#,
    )
    .await?;

    db.execute_unprepared(
        r#"CREATE TABLE IF NOT EXISTS solved_challenges (
            user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            challenge_name TEXT NOT NULL,
            ctf_name TEXT NOT NULL,
            solved_at TIMESTAMPTZ NOT NULL DEFAULT now(),
            PRIMARY KEY(user_id, challenge_name, ctf_name)
        )"#,
    )
    .await?;

    // Instance actions (spawn/stop/extend)
    db.execute_unprepared(
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
    .await?;

    // File downloads
    db.execute_unprepared(
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
    .await?;

    Ok(())
}
