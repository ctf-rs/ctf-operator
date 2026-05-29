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
    team_id: Option<&str>,
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
        team_id: Set(team_id.map(ToOwned::to_owned)),
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

/// Check whether any member of the team has solved this challenge.
pub async fn is_challenge_solved_for_team(
    db: &DatabaseConnection,
    team_id: &str,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<bool> {
    use sea_orm::ConnectionTrait;
    let result = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            db.get_database_backend(),
            r"SELECT COUNT(*) AS cnt
              FROM solved_challenges sc
              JOIN users u ON u.id = sc.user_id
              WHERE u.team_id = $1
                AND sc.challenge_name = $2
                AND sc.ctf_name = $3",
            vec![team_id.into(), challenge_name.into(), ctf_name.into()],
        ))
        .await?;

    let count: i64 = result.map_or(0, |r| r.try_get_by_index(0).unwrap_or(0));
    Ok(count > 0)
}

/// Check if a challenge is solved for the user or their team (whichever applies).
pub async fn is_challenge_solved(
    db: &DatabaseConnection,
    user_id: i64,
    team_id: Option<&str>,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<bool> {
    if let Some(tid) = team_id {
        is_challenge_solved_for_team(db, tid, challenge_name, ctf_name).await
    } else {
        is_challenge_solved_for_user(db, user_id, challenge_name, ctf_name).await
    }
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

/// List challenges solved by any member of the team.
pub async fn list_solved_challenges_for_team(
    db: &DatabaseConnection,
    team_id: &str,
) -> Result<Vec<SolvedKey>> {
    use sea_orm::ConnectionTrait;
    let raw_rows = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            db.get_database_backend(),
            r"SELECT DISTINCT sc.challenge_name, sc.ctf_name
              FROM solved_challenges sc
              JOIN users u ON u.id = sc.user_id
              WHERE u.team_id = $1",
            vec![team_id.into()],
        ))
        .await?;

    let rows: Vec<(String, String)> = raw_rows
        .into_iter()
        .filter_map(|r| {
            Some((
                r.try_get_by_index::<String>(0).ok()?,
                r.try_get_by_index::<String>(1).ok()?,
            ))
        })
        .collect();

    Ok(rows
        .into_iter()
        .map(|(challenge_name, ctf_name)| SolvedKey {
            challenge_name,
            ctf_name,
        })
        .collect())
}

/// List solved challenges for user or their team.
pub async fn list_solved_challenges(
    db: &DatabaseConnection,
    user_id: i64,
    team_id: Option<&str>,
) -> Result<Vec<SolvedKey>> {
    if let Some(tid) = team_id {
        list_solved_challenges_for_team(db, tid).await
    } else {
        list_solved_challenges_for_user(db, user_id).await
    }
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

/// Returns the names of flags any team member has solved for a given challenge.
pub async fn get_solved_flags_for_team(
    db: &DatabaseConnection,
    team_id: &str,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<Vec<String>> {
    Ok(flag_submissions::Entity::find()
        .select_only()
        .column(flag_submissions::Column::FlagName)
        .distinct()
        .filter(flag_submissions::Column::TeamId.eq(team_id))
        .filter(flag_submissions::Column::ChallengeName.eq(challenge_name))
        .filter(flag_submissions::Column::CtfName.eq(ctf_name))
        .filter(flag_submissions::Column::Accepted.eq(true))
        .into_tuple::<String>()
        .all(db)
        .await?)
}

/// Returns solved flag names for the user or their team.
pub async fn get_solved_flags(
    db: &DatabaseConnection,
    user_id: i64,
    team_id: Option<&str>,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<Vec<String>> {
    if let Some(tid) = team_id {
        get_solved_flags_for_team(db, tid, challenge_name, ctf_name).await
    } else {
        get_solved_flags_for_user(db, user_id, challenge_name, ctf_name).await
    }
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

/// Count how many distinct flags are solved for a challenge by any team member.
pub async fn count_solved_flags_for_team(
    db: &DatabaseConnection,
    team_id: &str,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<i64> {
    let flags = get_solved_flags_for_team(db, team_id, challenge_name, ctf_name).await?;
    Ok(flags.len() as i64)
}

/// Count solved flags for user or their team.
pub async fn count_solved_flags(
    db: &DatabaseConnection,
    user_id: i64,
    team_id: Option<&str>,
    challenge_name: &str,
    ctf_name: &str,
) -> Result<i64> {
    if let Some(tid) = team_id {
        count_solved_flags_for_team(db, tid, challenge_name, ctf_name).await
    } else {
        count_solved_flags_for_user(db, user_id, challenge_name, ctf_name).await
    }
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

/// Returns a map of (ctf_name, challenge_name) → count of distinct solved flags for a team.
pub async fn get_partial_solve_counts_for_team(
    db: &DatabaseConnection,
    team_id: &str,
) -> Result<std::collections::HashMap<(String, String), i64>> {
    let rows: Vec<(String, String, i64)> = flag_submissions::Entity::find()
        .select_only()
        .column(flag_submissions::Column::CtfName)
        .column(flag_submissions::Column::ChallengeName)
        .column_as(Expr::cust("COUNT(DISTINCT flag_name)"), "count")
        .filter(flag_submissions::Column::TeamId.eq(team_id))
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

/// Returns partial solve counts for user or their team.
pub async fn get_partial_solve_counts(
    db: &DatabaseConnection,
    user_id: i64,
    team_id: Option<&str>,
) -> Result<std::collections::HashMap<(String, String), i64>> {
    if let Some(tid) = team_id {
        get_partial_solve_counts_for_team(db, tid).await
    } else {
        get_partial_solve_counts_for_user(db, user_id).await
    }
}
