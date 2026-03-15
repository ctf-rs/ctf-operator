use std::{collections::HashMap, sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info};

use crate::state::AppState;

/// Key for flag in scoreboard: (ctf_name, challenge_name, flag_name)
pub type FlagKey = (String, String, String);

/// Key for challenge in scoreboard: (ctf_name, challenge_name)
pub type ChallengeKey = (String, String);

/// Key for user score: (ctf_name, user_id)
pub type UserScoreKey = (String, i64);

/// Cached flag scoring data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlagScore {
    pub base_points: u32,
    pub min_points: u32,
    pub decay: u32,
    pub solves: u32,
    pub current_points: u32,
}

impl FlagScore {
    pub fn new(base_points: u32, min_points: u32, decay: u32, solves: u32) -> Self {
        let current_points = Self::calculate(base_points, min_points, decay, solves);
        Self {
            base_points,
            min_points,
            decay,
            solves,
            current_points,
        }
    }

    fn calculate(base_points: u32, min_points: u32, decay: u32, solves: u32) -> u32 {
        if solves == 0 {
            return base_points;
        }
        let points = (base_points as f64) / (1.0 + (solves as f64 / decay as f64));
        points.round().max(min_points as f64) as u32
    }
}

/// User's score entry for leaderboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserScore {
    pub user_id: i64,
    pub username: String,
    pub total_points: u32,
    pub solves: u32,
    pub last_solve_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// The scoreboard cache
#[derive(Debug, Default)]
pub struct Scoreboard {
    /// Points per flag: (ctf, challenge, flag) -> FlagScore
    pub flag_scores: HashMap<FlagKey, FlagScore>,
    /// Total points per challenge: (ctf, challenge) -> total_points
    pub challenge_points: HashMap<ChallengeKey, u32>,
    /// User scores per CTF: (ctf, user_id) -> UserScore
    pub user_scores: HashMap<UserScoreKey, UserScore>,
    /// Sorted leaderboard per CTF: ctf_name -> sorted vec of UserScore
    pub leaderboards: HashMap<String, Vec<UserScore>>,
}

impl Scoreboard {
    pub fn get_challenge_points(&self, ctf_name: &str, challenge_name: &str) -> u32 {
        self.challenge_points
            .get(&(ctf_name.to_string(), challenge_name.to_string()))
            .copied()
            .unwrap_or(0)
    }

    pub fn get_flag_points(&self, ctf_name: &str, challenge_name: &str, flag_name: &str) -> u32 {
        self.flag_scores
            .get(&(
                ctf_name.to_string(),
                challenge_name.to_string(),
                flag_name.to_string(),
            ))
            .map(|s| s.current_points)
            .unwrap_or(0)
    }

    pub fn get_leaderboard(&self, ctf_name: &str) -> Vec<UserScore> {
        self.leaderboards.get(ctf_name).cloned().unwrap_or_default()
    }
}

/// Spawns the background scoreboard sync task
pub fn spawn_scoreboard_worker(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("scoreboard worker started");
        loop {
            if let Err(err) = refresh_scoreboard(&state).await {
                error!(error = %err, "failed to refresh scoreboard");
            }
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    })
}

/// Refresh the scoreboard cache from database
async fn refresh_scoreboard(state: &AppState) -> anyhow::Result<()> {
    let db = &state.db;
    let ctf_name = &state.config.ctf_name;

    // Get flag configurations from sync cache
    let flag_configs = get_flag_configs(state).await;

    // Get solve counts per flag from database
    let solve_counts = get_flag_solve_counts(db, ctf_name).await?;

    // Calculate flag scores
    let mut flag_scores: HashMap<FlagKey, FlagScore> = HashMap::new();
    let mut challenge_points: HashMap<ChallengeKey, u32> = HashMap::new();

    for ((challenge_name, flag_name), (base_points, min_points, decay)) in &flag_configs {
        let solves = solve_counts
            .get(&(challenge_name.clone(), flag_name.clone()))
            .copied()
            .unwrap_or(0);

        let score = FlagScore::new(*base_points, *min_points, *decay, solves);
        let key = (ctf_name.clone(), challenge_name.clone(), flag_name.clone());
        flag_scores.insert(key, score.clone());

        // Accumulate challenge points
        let ch_key = (ctf_name.clone(), challenge_name.clone());
        *challenge_points.entry(ch_key).or_insert(0) += score.current_points;
    }

    // Calculate user scores
    let user_scores = calculate_user_scores(db, ctf_name, &flag_scores).await?;

    // Build sorted leaderboard
    let mut leaderboard: Vec<UserScore> = user_scores.values().cloned().collect();
    leaderboard.sort_by(|a, b| {
        b.total_points
            .cmp(&a.total_points)
            .then_with(|| a.last_solve_at.cmp(&b.last_solve_at))
    });

    // Update cache
    let mut cache = state.scoreboard.write().await;
    cache.flag_scores = flag_scores;
    cache.challenge_points = challenge_points;
    cache.user_scores = user_scores;
    cache.leaderboards.insert(ctf_name.clone(), leaderboard);

    Ok(())
}

/// Get flag configurations from the sync cache
async fn get_flag_configs(state: &AppState) -> HashMap<(String, String), (u32, u32, u32)> {
    // Read from challenges table which has flag info
    let mut configs = HashMap::new();

    // Query challenges with their flags
    let rows = sqlx::query_as::<_, (String, sqlx::types::Json<Vec<crate::db::FlagView>>)>(
        r#"SELECT name, flags_json FROM app_challenges WHERE ctf_name = $1"#,
    )
    .bind(&state.config.ctf_name)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    for (challenge_name, flags) in rows {
        for flag in flags.0 {
            configs.insert(
                (challenge_name.clone(), flag.name.clone()),
                (flag.base_points, flag.min_points, flag.decay),
            );
        }
    }

    configs
}

/// Get solve counts per flag from database
async fn get_flag_solve_counts(
    db: &PgPool,
    ctf_name: &str,
) -> anyhow::Result<HashMap<(String, String), u32>> {
    let rows = sqlx::query_as::<_, (String, String, i64)>(
        r#"
        SELECT challenge_name, flag_name, COUNT(DISTINCT user_id) as solve_count
        FROM app_flag_submissions
        WHERE ctf_name = $1 AND accepted = true
        GROUP BY challenge_name, flag_name
        "#,
    )
    .bind(ctf_name)
    .fetch_all(db)
    .await?;

    Ok(rows
        .into_iter()
        .map(|(ch, fl, cnt)| ((ch, fl), cnt as u32))
        .collect())
}

/// Calculate user scores based on their solves
async fn calculate_user_scores(
    db: &PgPool,
    ctf_name: &str,
    flag_scores: &HashMap<FlagKey, FlagScore>,
) -> anyhow::Result<HashMap<UserScoreKey, UserScore>> {
    // Get all accepted submissions with user info
    let rows = sqlx::query_as::<_, (i64, String, String, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT DISTINCT ON (s.user_id, s.challenge_name, s.flag_name)
            s.user_id, 
            COALESCE(u.name, u.sub) as username,
            s.challenge_name, 
            s.flag_name,
            s.created_at
        FROM app_flag_submissions s
        JOIN app_users u ON u.id = s.user_id
        WHERE s.ctf_name = $1 AND s.accepted = true
        ORDER BY s.user_id, s.challenge_name, s.flag_name, s.created_at
        "#,
    )
    .bind(ctf_name)
    .fetch_all(db)
    .await?;

    let mut user_scores: HashMap<UserScoreKey, UserScore> = HashMap::new();

    for (user_id, username, challenge_name, flag_name, solved_at) in rows {
        let key = (ctf_name.to_string(), user_id);
        let flag_key = (ctf_name.to_string(), challenge_name, flag_name);

        let points = flag_scores
            .get(&flag_key)
            .map(|s| s.current_points)
            .unwrap_or(0);

        let entry = user_scores.entry(key).or_insert_with(|| UserScore {
            user_id,
            username: username.clone(),
            total_points: 0,
            solves: 0,
            last_solve_at: None,
        });

        entry.total_points += points;
        entry.solves += 1;
        match entry.last_solve_at {
            None => entry.last_solve_at = Some(solved_at),
            Some(last) if solved_at > last => entry.last_solve_at = Some(solved_at),
            _ => {}
        }
    }

    Ok(user_scores)
}
