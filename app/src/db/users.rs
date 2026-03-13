use anyhow::Result;
use sqlx::PgPool;

use super::models::{User, UserRow, user_from_row};

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
