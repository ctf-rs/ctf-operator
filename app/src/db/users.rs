use anyhow::Result;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, Set,
    sea_query::OnConflict,
};

use super::entity::{sessions, users};
use super::models::User;

pub async fn upsert_user(
    db: &DatabaseConnection,
    sub: &str,
    email: Option<&str>,
    name: Option<&str>,
) -> Result<User> {
    let model = users::ActiveModel {
        sub: Set(sub.to_owned()),
        email: Set(email.map(ToOwned::to_owned)),
        name: Set(name.map(ToOwned::to_owned)),
        ..Default::default()
    };

    let result = users::Entity::insert(model)
        .on_conflict(
            OnConflict::column(users::Column::Sub)
                .update_columns([users::Column::Email, users::Column::Name])
                .to_owned(),
        )
        .exec_with_returning(db)
        .await?;

    Ok(User {
        id: result.id,
        sub: result.sub,
        email: result.email,
        name: result.name,
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
            Ok(Some(User {
                id: user.id,
                sub: user.sub,
                email: user.email,
                name: user.name,
                roles,
            }))
        }
        _ => Ok(None),
    }
}
