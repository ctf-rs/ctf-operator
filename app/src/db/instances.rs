use anyhow::Result;
use sqlx::PgPool;

use super::models::{InstanceRow, InstanceView};

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

pub async fn clear_instance_for_user(
    db: &PgPool,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<()> {
    sqlx::query(
        r#"DELETE FROM app_instances
           WHERE user_id = $1 AND challenge_name = $2 AND ctf_name = $3"#,
    )
    .bind(user_id)
    .bind(challenge_name)
    .bind(ctf_name)
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
