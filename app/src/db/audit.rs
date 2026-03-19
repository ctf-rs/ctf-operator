use anyhow::Result;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set};

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
    ignore_case: bool,
    ignore_random: bool,
) -> Result<()> {
    let model = instance_flags::ActiveModel {
        instance_name: Set(instance_name.to_owned()),
        user_id: Set(user_id),
        challenge_name: Set(challenge_name.to_owned()),
        ctf_name: Set(ctf_name.to_owned()),
        flag_name: Set(flag_name.to_owned()),
        generated_flag: Set(generated_flag.to_owned()),
        ignore_case: Set(ignore_case),
        ignore_random: Set(ignore_random),
        ..Default::default()
    };

    instance_flags::Entity::insert(model).exec(db).await?;
    Ok(())
}

/// Information about a matching generated flag found in the database.
#[derive(Debug)]
pub struct FlagOrigin {
    /// Primary key of the `instance_flags` row.
    pub id: i64,
    /// The user who owned the instance that generated this flag (if known).
    pub user_id: Option<i64>,
    /// The challenge the flag was generated for.
    pub challenge_name: String,
}

/// Look up a submitted flag value in `instance_flags` to determine its origin.
///
/// Returns [`FlagOrigin`] if the value matches any previously generated flag
/// for the given CTF, regardless of which user or challenge it belongs to.
pub async fn check_flag_origin(
    db: &DatabaseConnection,
    submitted_flag: &str,
    ctf_name: &str,
) -> Result<Option<FlagOrigin>> {
    let row = instance_flags::Entity::find()
        .filter(instance_flags::Column::GeneratedFlag.eq(submitted_flag))
        .filter(instance_flags::Column::CtfName.eq(ctf_name))
        .one(db)
        .await?;

    Ok(row.map(|m| FlagOrigin {
        id: m.id,
        user_id: m.user_id,
        challenge_name: m.challenge_name,
    }))
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
