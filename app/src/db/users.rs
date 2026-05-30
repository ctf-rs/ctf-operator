use anyhow::Result;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set, sea_query::OnConflict,
};

use super::entity::{sessions, teams, users};
use super::models::User;

/// Link a Discord account to a user by user id.
pub async fn link_discord(
    db: &DatabaseConnection,
    user_id: i64,
    discord_id: &str,
    discord_username: &str,
) -> Result<()> {
    let mut model: users::ActiveModel = users::Entity::find_by_id(user_id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("user not found"))?
        .into();
    model.discord_id = Set(Some(discord_id.to_owned()));
    model.discord_username = Set(Some(discord_username.to_owned()));
    users::Entity::update(model).exec(db).await?;
    Ok(())
}

/// Ensure the `teams` row exists for the given team_id, creating it with
/// `team_name = team_id` when first seen.
pub async fn ensure_team(db: &DatabaseConnection, team_id: &str) -> Result<()> {
    let model = teams::ActiveModel {
        team_id: Set(team_id.to_owned()),
        team_name: Set(team_id.to_owned()),
        ..Default::default()
    };
    teams::Entity::insert(model)
        .on_conflict(
            OnConflict::column(teams::Column::TeamId)
                .do_nothing()
                .to_owned(),
        )
        .do_nothing()
        .exec(db)
        .await?;
    Ok(())
}

pub async fn upsert_user(
    db: &DatabaseConnection,
    sub: &str,
    email: Option<&str>,
    name: Option<&str>,
    team_id: Option<&str>,
) -> Result<User> {
    // Auto-create team entry when a new team_id appears.
    if let Some(tid) = team_id {
        ensure_team(db, tid).await?;
    }

    let model = users::ActiveModel {
        sub: Set(sub.to_owned()),
        email: Set(email.map(ToOwned::to_owned)),
        name: Set(name.map(ToOwned::to_owned)),
        team_id: Set(team_id.map(ToOwned::to_owned)),
        ..Default::default()
    };

    let result = users::Entity::insert(model)
        .on_conflict(
            OnConflict::column(users::Column::Sub)
                .update_columns([
                    users::Column::Email,
                    users::Column::Name,
                    users::Column::TeamId,
                ])
                .to_owned(),
        )
        .exec_with_returning(db)
        .await?;

    // Resolve team_name from teams table
    let team_name = if let Some(ref tid) = result.team_id {
        teams::Entity::find_by_id(tid.clone())
            .one(db)
            .await?
            .map(|t| t.team_name)
    } else {
        None
    };

    Ok(User {
        id: result.id,
        sub: result.sub,
        email: result.email,
        name: result.name,
        discord_id: result.discord_id,
        discord_username: result.discord_username,
        team_id: result.team_id,
        team_name,
        roles: Vec::new(),
    })
}

pub async fn create_session(
    db: &DatabaseConnection,
    token: &str,
    user_id: i64,
    roles: &[String],
) -> Result<()> {
    let model = sessions::ActiveModel {
        token: Set(token.to_owned()),
        user_id: Set(user_id),
        roles_json: Set(serde_json::json!(roles)),
        ..Default::default()
    };

    sessions::Entity::insert(model)
        .on_conflict(
            OnConflict::column(sessions::Column::Token)
                .update_columns([sessions::Column::UserId, sessions::Column::RolesJson])
                .to_owned(),
        )
        .exec(db)
        .await?;

    Ok(())
}

pub async fn delete_session(db: &DatabaseConnection, token: &str) -> Result<()> {
    sessions::Entity::delete_many()
        .filter(sessions::Column::Token.eq(token))
        .exec(db)
        .await?;
    Ok(())
}

pub async fn get_user_by_session(db: &DatabaseConnection, token: &str) -> Result<Option<User>> {
    let result = sessions::Entity::find()
        .filter(sessions::Column::Token.eq(token))
        .find_also_related(users::Entity)
        .one(db)
        .await?;

    match result {
        Some((session, Some(user))) => {
            let roles: Vec<String> = serde_json::from_value(session.roles_json).unwrap_or_default();

            // Resolve team_name from teams table
            let team_name = if let Some(ref tid) = user.team_id {
                teams::Entity::find_by_id(tid.clone())
                    .one(db)
                    .await?
                    .map(|t| t.team_name)
            } else {
                None
            };

            Ok(Some(User {
                id: user.id,
                sub: user.sub,
                email: user.email,
                name: user.name,
                discord_id: user.discord_id,
                discord_username: user.discord_username,
                team_id: user.team_id,
                team_name,
                roles,
            }))
        }
        _ => Ok(None),
    }
}
