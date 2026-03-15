use anyhow::Result;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, Set, Statement};

use super::entity::{downloads, instance_actions, instance_flags};

/// Record a generated flag from K8s Flag resource
pub async fn log_flag_generation(
    db: &DatabaseConnection,
    instance_name: &str,
    user_id: Option<i64>,
    challenge_name: &str,
    ctf_name: &str,
    flag_name: &str,
    generated_flag: &str,
) -> Result<()> {
    let model = instance_flags::ActiveModel {
        instance_name: Set(instance_name.to_owned()),
        user_id: Set(user_id),
        challenge_name: Set(challenge_name.to_owned()),
        ctf_name: Set(ctf_name.to_owned()),
        flag_name: Set(flag_name.to_owned()),
        generated_flag: Set(generated_flag.to_owned()),
        ..Default::default()
    };

    instance_flags::Entity::insert(model).exec(db).await?;
    Ok(())
}

/// Check if a submitted flag was generated for a different user (cheat detection)
/// Returns the instance_flag id if the flag belongs to another user
pub async fn check_flag_cheated(
    db: &DatabaseConnection,
    submitted_flag: &str,
    ctf_name: &str,
    submitting_user_id: i64,
) -> Result<Option<i64>> {
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"SELECT id
               FROM instance_flags
               WHERE generated_flag = $1
                 AND ctf_name = $2
                 AND (user_id IS NULL OR user_id != $3)
               LIMIT 1"#,
            [
                submitted_flag.into(),
                ctf_name.into(),
                submitting_user_id.into(),
            ],
        ))
        .await?;

    if let Some(row) = result {
        let flag_gen_id: i64 = row.try_get_by_index(0)?;
        return Ok(Some(flag_gen_id));
    }

    Ok(None)
}

/// Log an instance action (spawn/stop/extend)
pub async fn log_instance_action(
    db: &DatabaseConnection,
    user_id: i64,
    instance_name: &str,
    challenge_name: &str,
    ctf_name: &str,
    action: &str,
    ip_address: Option<&str>,
) -> Result<()> {
    let model = instance_actions::ActiveModel {
        user_id: Set(user_id),
        instance_name: Set(instance_name.to_owned()),
        challenge_name: Set(challenge_name.to_owned()),
        ctf_name: Set(ctf_name.to_owned()),
        action: Set(action.to_owned()),
        ip_address: Set(ip_address.map(ToOwned::to_owned)),
        ..Default::default()
    };

    instance_actions::Entity::insert(model).exec(db).await?;
    Ok(())
}

/// Log a file download
pub async fn log_download(
    db: &DatabaseConnection,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    file_url: &str,
    ip_address: Option<&str>,
) -> Result<()> {
    let model = downloads::ActiveModel {
        user_id: Set(user_id),
        challenge_name: Set(challenge_name.to_owned()),
        ctf_name: Set(ctf_name.to_owned()),
        file_url: Set(file_url.to_owned()),
        ip_address: Set(ip_address.map(ToOwned::to_owned)),
        ..Default::default()
    };

    downloads::Entity::insert(model).exec(db).await?;
    Ok(())
}
