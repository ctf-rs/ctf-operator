use anyhow::Result;
use sqlx::PgPool;

use super::models::{User, UserRow, user_from_row};

pub async fn upsert_user(
    db: &PgPool,
    sub: &str,
    email: Option<&str>,
    name: Option<&str>,
) -> Result<User> {
    let row = sqlx::query_as::<_, UserRow>(
        r#"INSERT INTO users(sub, email, name)
           VALUES ($1, $2, $3)
           ON CONFLICT(sub) DO UPDATE
             SET email = EXCLUDED.email,
                 name = EXCLUDED.name
           RETURNING id, sub, email, name"#,
    )
    .bind(sub)
    .bind(email)
    .bind(name)
    .fetch_one(db)
    .await?;

    Ok(user_from_row(row))
}

pub async fn create_session(
    db: &PgPool,
    token: &str,
    user_id: i64,
    roles: &[String],
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO sessions(token, user_id, roles_json)
           VALUES ($1, $2, $3)
           ON CONFLICT(token) DO UPDATE
             SET user_id = EXCLUDED.user_id,
                 roles_json = EXCLUDED.roles_json"#,
    )
    .bind(token)
    .bind(user_id)
    .bind(sqlx::types::Json(roles.to_vec()))
    .execute(db)
    .await?;

    Ok(())
}

pub async fn delete_session(db: &PgPool, token: &str) -> Result<()> {
    sqlx::query("DELETE FROM sessions WHERE token = $1")
        .bind(token)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_user_by_session(db: &PgPool, token: &str) -> Result<Option<User>> {
    let row = sqlx::query_as::<
        _,
        (
            i64,
            String,
            Option<String>,
            Option<String>,
            sqlx::types::Json<Vec<String>>,
        ),
    >(
        r#"SELECT u.id, u.sub, u.email, u.name, s.roles_json
           FROM users u
           INNER JOIN sessions s ON s.user_id = u.id
           WHERE s.token = $1"#,
    )
    .bind(token)
    .fetch_optional(db)
    .await?;

    Ok(row.map(|(id, sub, email, name, roles_json)| User {
        id,
        sub,
        email,
        name,
        roles: roles_json.0,
    }))
}
