use anyhow::Result;
use sqlx::PgPool;

use super::models::SolvedKey;

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

pub async fn mark_challenge_solved(
    db: &PgPool,
    user_id: i64,
    challenge_namespace: &str,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_solved_challenges(
            user_id, challenge_namespace, challenge_name, ctf_name
        ) VALUES ($1, $2, $3, $4)
        ON CONFLICT(user_id, challenge_namespace, challenge_name, ctf_name)
        DO UPDATE SET solved_at = now()"#,
    )
    .bind(user_id)
    .bind(challenge_namespace)
    .bind(challenge_name)
    .bind(ctf_name)
    .execute(db)
    .await?;

    Ok(())
}

pub async fn is_challenge_solved_for_user(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<bool> {
    let solved = sqlx::query_scalar::<_, bool>(
        r#"SELECT EXISTS (
            SELECT 1 FROM app_solved_challenges
            WHERE user_id = $1 AND challenge_name = $2 AND ctf_name = $3
        )"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .fetch_one(db)
    .await?;

    Ok(solved)
}

pub async fn list_solved_challenges_for_user(db: &PgPool, user_id: i64) -> Result<Vec<SolvedKey>> {
    let rows = sqlx::query_as::<_, (String, String)>(
        r#"SELECT challenge_name, ctf_name
           FROM app_solved_challenges
           WHERE user_id = $1"#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(challenge_name, ctf_name)| SolvedKey {
            challenge_name,
            ctf_name,
        })
        .collect())
}

/// Returns the names of flags this user has already solved for a given challenge.
pub async fn get_solved_flags_for_user(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<Vec<String>> {
    let rows = sqlx::query_scalar::<_, String>(
        r#"SELECT DISTINCT flag_name
           FROM app_flag_submissions
           WHERE user_id = $1 AND challenge_name = $2 AND ctf_name = $3 AND accepted = true"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .fetch_all(db)
    .await?;

    Ok(rows)
}

/// Count how many flags are solved for a challenge by a user.
pub async fn count_solved_flags_for_user(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<i64> {
    let count = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(DISTINCT flag_name)
           FROM app_flag_submissions
           WHERE user_id = $1 AND challenge_name = $2 AND ctf_name = $3 AND accepted = true"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .fetch_one(db)
    .await?;

    Ok(count)
}

/// Returns a map of (ctf_name, challenge_name) → count of solved flags for a user.
/// Used to detect partial solves on the index page.
pub async fn get_partial_solve_counts_for_user(
    db: &PgPool,
    user_id: i64,
) -> Result<std::collections::HashMap<(String, String), i64>> {
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        r#"SELECT ctf_name, challenge_name, COUNT(DISTINCT flag_name)
           FROM app_flag_submissions
           WHERE user_id = $1 AND accepted = true
           GROUP BY ctf_name, challenge_name"#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(ctf, challenge, count)| ((ctf, challenge), count))
        .collect())
}
