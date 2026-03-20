use anyhow::Result;
use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QuerySelect, Set,
    sea_query::{Expr, OnConflict},
};

use super::entity::{flag_submissions, solved_challenges};
use super::models::SolvedKey;

pub async fn add_flag_submission(
    db: &DatabaseConnection,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
    flag_name: &str,
    submitted_flag: &str,
    accepted: bool,
    cheated: bool,
    cheated_instance_flag_id: Option<i64>,
    admin_submission: bool,
    ip_address: Option<&str>,
) -> Result<()> {
    let model = flag_submissions::ActiveModel {
        user_id: Set(user_id),
        challenge_name: Set(challenge_name.to_owned()),
        ctf_name: Set(ctf_name.to_owned()),
        flag_name: Set(flag_name.to_owned()),
        submitted_flag: Set(submitted_flag.to_owned()),
        accepted: Set(accepted),
        cheated: Set(cheated),
        cheated_instance_flag_id: Set(cheated_instance_flag_id),
        admin_submission: Set(admin_submission),
        ip_address: Set(ip_address.map(ToOwned::to_owned)),
        ..Default::default()
    };

    flag_submissions::Entity::insert(model).exec(db).await?;
    Ok(())
}

pub async fn mark_challenge_solved(
    db: &DatabaseConnection,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<()> {
    let model = solved_challenges::ActiveModel {
        user_id: Set(user_id),
        challenge_name: Set(challenge_name.to_owned()),
        ctf_name: Set(ctf_name.to_owned()),
        ..Default::default()
    };

    solved_challenges::Entity::insert(model)
        .on_conflict(
            OnConflict::columns([
                solved_challenges::Column::UserId,
                solved_challenges::Column::ChallengeName,
                solved_challenges::Column::CtfName,
            ])
            .value(
                solved_challenges::Column::SolvedAt,
                sea_orm::sea_query::Expr::current_timestamp(),
            )
            .to_owned(),
        )
        .exec(db)
        .await?;

    Ok(())
}

pub async fn is_challenge_solved_for_user(
    db: &DatabaseConnection,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<bool> {
    let count = solved_challenges::Entity::find()
        .filter(solved_challenges::Column::UserId.eq(user_id))
        .filter(solved_challenges::Column::ChallengeName.eq(challenge_name))
        .filter(solved_challenges::Column::CtfName.eq(ctf_name))
        .count(db)
        .await?;

    Ok(count > 0)
}

pub async fn list_solved_challenges_for_user(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<Vec<SolvedKey>> {
    let rows = solved_challenges::Entity::find()
        .filter(solved_challenges::Column::UserId.eq(user_id))
        .all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|r| SolvedKey {
            challenge_name: r.challenge_name,
            ctf_name: r.ctf_name,
        })
        .collect())
}

/// Returns the names of flags this user has already solved for a given challenge.
pub async fn get_solved_flags_for_user(
    db: &DatabaseConnection,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<Vec<String>> {
    Ok(flag_submissions::Entity::find()
        .select_only()
        .column(flag_submissions::Column::FlagName)
        .distinct()
        .filter(flag_submissions::Column::UserId.eq(user_id))
        .filter(flag_submissions::Column::ChallengeName.eq(challenge_name))
        .filter(flag_submissions::Column::CtfName.eq(ctf_name))
        .filter(flag_submissions::Column::Accepted.eq(true))
        .into_tuple::<String>()
        .all(db)
        .await?)
}

/// Count how many distinct flags are solved for a challenge by a user.
pub async fn count_solved_flags_for_user(
    db: &DatabaseConnection,
    user_id: i64,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<i64> {
    let flags = get_solved_flags_for_user(db, user_id, challenge_name, ctf_name).await?;
    Ok(flags.len() as i64)
}

/// Returns a map of (ctf_name, challenge_name) → count of distinct solved flags for a user.
/// Used to detect partial solves on the index page.
pub async fn get_partial_solve_counts_for_user(
    db: &DatabaseConnection,
    user_id: i64,
) -> Result<std::collections::HashMap<(String, String), i64>> {
    let rows: Vec<(String, String, i64)> = flag_submissions::Entity::find()
        .select_only()
        .column(flag_submissions::Column::CtfName)
        .column(flag_submissions::Column::ChallengeName)
        .column_as(Expr::cust("COUNT(DISTINCT flag_name)"), "count")
        .filter(flag_submissions::Column::UserId.eq(user_id))
        .filter(flag_submissions::Column::Accepted.eq(true))
        .group_by(flag_submissions::Column::CtfName)
        .group_by(flag_submissions::Column::ChallengeName)
        .into_tuple()
        .all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(ctf, challenge, count)| ((ctf, challenge), count))
        .collect())
}
