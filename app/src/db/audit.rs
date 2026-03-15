use anyhow::Result;
use sqlx::PgPool;

/// Record a generated flag from K8s Flag resource
pub async fn log_flag_generation(
    db: &PgPool,
    instance_name: &str,
    user_id: Option<i64>,
    challenge_name: &str,
    ctf_name: &str,
    flag_name: &str,
    generated_flag: &str,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_audit_flag_gen(
            instance_name, user_id, challenge_name, ctf_name, flag_name, generated_flag
        ) VALUES ($1, $2, $3, $4, $5, $6)
        ON CONFLICT DO NOTHING"#,
    )
    .bind(instance_name)
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .bind(flag_name)
    .bind(generated_flag)
    .execute(db)
    .await?;

    Ok(())
}

/// Check if a submitted flag was generated for a different user (cheat detection)
/// Returns (flag_gen_id, cheated_from_user_id) if the flag belongs to another user
pub async fn check_flag_cheated(
    db: &PgPool,
    submitted_flag: &str,
    ctf_name: &str,
    submitting_user_id: i64,
) -> Result<Option<(i64, i64)>> {
    let result = sqlx::query_as::<_, (i64, Option<i64>)>(
        r#"SELECT id, user_id
           FROM app_audit_flag_gen
           WHERE generated_flag = $1
             AND ctf_name = $2
             AND (user_id IS NULL OR user_id != $3)
           LIMIT 1"#,
    )
    .bind(submitted_flag)
    .bind(ctf_name)
    .bind(submitting_user_id)
    .fetch_optional(db)
    .await?;

    if let Some((flag_gen_id, Some(owner_user_id))) = result {
        Ok(Some((flag_gen_id, owner_user_id)))
    } else {
        Ok(None)
    }
}

/// Log an instance action (spawn/stop/extend)
pub async fn log_instance_action(
    db: &PgPool,
    user_id: i64,
    instance_name: &str,
    challenge_name: &str,
    ctf_name: &str,
    action: &str,
    ip_address: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_audit_instance(
            user_id, instance_name, challenge_name, ctf_name, action, ip_address
        ) VALUES ($1, $2, $3, $4, $5, $6)"#,
    )
    .bind(user_id)
    .bind(instance_name)
    .bind(challenge_name)
    .bind(ctf_name)
    .bind(action)
    .bind(ip_address)
    .execute(db)
    .await?;

    Ok(())
}

/// Log a flag submission with cheat detection info
pub async fn log_submission(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
    cheated: bool,
    cheated_from_user_id: Option<i64>,
    cheated_flag_gen_id: Option<i64>,
    ip_address: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_audit_submission(
            user_id, challenge_name, ctf_name, flag_name, submitted_flag,
            accepted, cheated, cheated_from_user_id, cheated_flag_gen_id, ip_address
        ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .bind(flag_name)
    .bind(submitted_flag)
    .bind(accepted)
    .bind(cheated)
    .bind(cheated_from_user_id)
    .bind(cheated_flag_gen_id)
    .bind(ip_address)
    .execute(db)
    .await?;

    Ok(())
}

/// Log a file download
pub async fn log_download(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    file_url: &str,
    ip_address: Option<&str>,
) -> Result<()> {
    sqlx::query(
        r#"INSERT INTO app_audit_download(
            user_id, challenge_name, ctf_name, file_url, ip_address
        ) VALUES ($1, $2, $3, $4, $5)"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
    .bind(file_url)
    .bind(ip_address)
    .execute(db)
    .await?;

    Ok(())
}
