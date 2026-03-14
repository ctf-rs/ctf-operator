use anyhow::Result;
use sqlx::PgPool;

use super::models::{ChallengeRow, ChallengeView, challenge_from_row};

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
                namespace, name, ctf_name, ctf_public, full_name, description, categories_json, difficulty, files_json, flags_json, exposed_ports_json
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT(namespace, name, ctf_name) DO UPDATE
              SET ctf_public = EXCLUDED.ctf_public,
                  full_name = EXCLUDED.full_name,
                  description = EXCLUDED.description,
                  categories_json = EXCLUDED.categories_json,
                  difficulty = EXCLUDED.difficulty,
                  files_json = EXCLUDED.files_json,
                  flags_json = EXCLUDED.flags_json,
                  exposed_ports_json = EXCLUDED.exposed_ports_json,
                  updated_at = now()"#,
        )
        .bind(&row.namespace)
        .bind(&row.name)
        .bind(&row.ctf_name)
        .bind(row.ctf_public)
        .bind(&row.full_name)
        .bind(&row.description)
        .bind(sqlx::types::Json(&row.categories))
        .bind(row.difficulty.map(|d| serde_json::to_value(d).ok().and_then(|v| v.as_str().map(String::from))).flatten())
        .bind(sqlx::types::Json(&row.files))
        .bind(sqlx::types::Json(&row.flags))
        .bind(sqlx::types::Json(&row.exposed_ports))
        .execute(db)
        .await?;
    }

    Ok(())
}

pub async fn list_challenges_for_ctf(db: &PgPool, ctf_name: &str) -> Result<Vec<ChallengeView>> {
    let rows = sqlx::query_as::<_, ChallengeRow>(
        r#"SELECT namespace, name, ctf_name, ctf_public, full_name, description, categories_json, difficulty, files_json, flags_json, exposed_ports_json
           FROM app_challenges
           WHERE ctf_name = $1
           ORDER BY full_name ASC"#,
    )
    .bind(ctf_name)
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
        r#"SELECT namespace, name, ctf_name, ctf_public, full_name, description, categories_json, difficulty, files_json, flags_json, exposed_ports_json
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
