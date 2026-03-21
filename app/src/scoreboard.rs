use std::{collections::HashMap, sync::Arc, time::Duration};

use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityTrait, QueryFilter,
    QuerySelect, Statement, sea_query::Expr,
};
use serde::{Deserialize, Serialize};
use tracing::{error, info, warn};

use crate::db::entity::flag_submissions;
use crate::{metrics, state::AppState};

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

        // Shift solves so the first solve keeps full base points.
        let shifted_solves = solves.saturating_sub(1) as f64;
        let decay = f64::from(decay.max(1));
        let progress = shifted_solves / decay;

        // Mixed linear + quadratic decay for stronger early drop while
        // preserving a smooth curve to the minimum floor.
        let factor = (0.7 * progress) + (0.3 * progress * progress);
        let value =
            f64::from(base_points) + (f64::from(min_points) - f64::from(base_points)) * factor;

        value.ceil().max(f64::from(min_points)) as u32
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

// ---------------------------------------------------------------------------
// Serializable snapshot (for DB persistence)
// ---------------------------------------------------------------------------

/// A JSON-friendly snapshot of the scoreboard for one CTF.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ScoreboardSnapshot {
    flag_scores: Vec<FlagScoreEntry>,
    /// Pre-computed total points per challenge.
    #[serde(default)]
    challenge_points: HashMap<String, u32>,
    user_scores: Vec<UserScore>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FlagScoreEntry {
    challenge: String,
    flag: String,
    score: FlagScore,
}

impl Scoreboard {
    /// Build a snapshot for a single CTF.
    fn snapshot(&self, ctf_name: &str) -> ScoreboardSnapshot {
        let flag_scores = self
            .flag_scores
            .iter()
            .filter(|((ctf, _, _), _)| ctf == ctf_name)
            .map(|((_, ch, fl), score)| FlagScoreEntry {
                challenge: ch.clone(),
                flag: fl.clone(),
                score: score.clone(),
            })
            .collect();

        let challenge_points: HashMap<String, u32> = self
            .challenge_points
            .iter()
            .filter(|((ctf, _), _)| ctf == ctf_name)
            .map(|((_, ch), pts)| (ch.clone(), *pts))
            .collect();

        let user_scores = self.leaderboards.get(ctf_name).cloned().unwrap_or_default();

        ScoreboardSnapshot {
            flag_scores,
            challenge_points,
            user_scores,
        }
    }

    /// Restore from a snapshot for a given CTF (additive – does not clear other CTFs).
    fn apply_snapshot(&mut self, ctf_name: &str, snap: ScoreboardSnapshot) {
        for entry in &snap.flag_scores {
            let key = (
                ctf_name.to_string(),
                entry.challenge.clone(),
                entry.flag.clone(),
            );
            self.flag_scores.insert(key, entry.score.clone());
        }

        // Restore challenge_points from snapshot.
        for (ch, pts) in &snap.challenge_points {
            let ch_key = (ctf_name.to_string(), ch.clone());
            self.challenge_points.insert(ch_key, *pts);
        }

        // Restore user scores
        for us in &snap.user_scores {
            let key = (ctf_name.to_string(), us.user_id);
            self.user_scores.insert(key, us.clone());
        }

        self.leaderboards
            .insert(ctf_name.to_string(), snap.user_scores);
    }
}

/// Persist the current scoreboard cache to the database.
async fn save_scoreboard_cache(
    db: &DatabaseConnection,
    ctf_name: &str,
    snapshot: &ScoreboardSnapshot,
) -> anyhow::Result<()> {
    let json = serde_json::to_value(snapshot)?;
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"INSERT INTO scoreboard_cache (ctf_name, data, updated_at)
           VALUES ($1, $2, now())
           ON CONFLICT (ctf_name) DO UPDATE SET data = $2, updated_at = now()"#,
        [ctf_name.into(), json.into()],
    ))
    .await?;
    Ok(())
}

/// Load a previously persisted scoreboard snapshot from the database.
async fn load_scoreboard_cache(
    db: &DatabaseConnection,
    ctf_name: &str,
) -> anyhow::Result<Option<ScoreboardSnapshot>> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"SELECT data FROM scoreboard_cache WHERE ctf_name = $1"#,
            [ctf_name.into()],
        ))
        .await?;
    match row {
        Some(r) => {
            let json: serde_json::Value = r.try_get_by_index(0)?;
            let snap: ScoreboardSnapshot = serde_json::from_value(json)?;
            Ok(Some(snap))
        }
        None => Ok(None),
    }
}

/// Spawns the background scoreboard sync task
pub fn spawn_scoreboard_worker(state: Arc<AppState>) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        info!("scoreboard worker started");

        // Seed the in-memory cache from DB so a restart while frozen keeps old scores.
        let ctf_name = &state.config.ctf_name;
        match load_scoreboard_cache(&state.db, ctf_name).await {
            Ok(Some(snap)) => {
                let mut cache = state.scoreboard.write().await;
                cache.apply_snapshot(ctf_name, snap);
                info!("scoreboard cache restored from database");
            }
            Ok(None) => {
                info!("no persisted scoreboard cache found, starting fresh");
            }
            Err(err) => {
                warn!(error = %err, "failed to load scoreboard cache from database");
            }
        }

        loop {
            if let Err(err) = refresh_scoreboard(&state).await {
                error!(error = %err, "failed to refresh scoreboard");
            }
            update_instance_gauges(&state);
            tokio::time::sleep(Duration::from_secs(10)).await;
        }
    })
}

/// Update the `active_instances` and `active_users` gauges from the k8s
/// instance Store.  Runs on the scoreboard timer (every 10 s).
fn update_instance_gauges(state: &AppState) {
    use std::collections::HashSet;

    let instances: Vec<_> = state.instances.state();
    let mut per_challenge: HashMap<String, u64> = HashMap::new();
    let mut active_user_ids: HashSet<i64> = HashSet::new();

    for inst in &instances {
        let challenge = inst.spec.challenge.clone();
        *per_challenge.entry(challenge).or_default() += 1;
        if let Some(ref user) = inst.spec.user {
            active_user_ids.insert(user.id);
        }
    }

    let m = metrics::get();
    for (challenge, count) in &per_challenge {
        m.active_instances
            .record(*count, &m.attrs(&[metrics::challenge_kv(challenge)]));
    }
    m.active_users
        .record(active_user_ids.len() as u64, &m.attrs(&[]));
}

/// Refresh the scoreboard cache from database
async fn refresh_scoreboard(state: &AppState) -> anyhow::Result<()> {
    let sb_config = crate::helpers::get_ctf_scoreboard_config(state);
    let frozen = crate::helpers::is_scoreboard_frozen(&sb_config);

    let db = &state.db;
    let ctf_name = &state.config.ctf_name;

    // ── Frozen: reload the last-persisted snapshot from DB and return ──
    if frozen {
        match load_scoreboard_cache(db, ctf_name).await {
            Ok(Some(snap)) => {
                let mut cache = state.scoreboard.write().await;
                cache.apply_snapshot(ctf_name, snap);
            }
            Ok(None) => {
                // No snapshot yet — fall through and compute one so the
                // page is not blank on the very first freeze.
                return compute_and_persist(state, db, ctf_name).await;
            }
            Err(err) => {
                warn!(error = %err, "failed to reload scoreboard cache while frozen");
            }
        }
        return Ok(());
    }

    // ── Not frozen: compute fresh scores and persist ──
    compute_and_persist(state, db, ctf_name).await
}

/// Compute flag scores, challenge points, leaderboard and persist to DB.
async fn compute_and_persist(
    state: &AppState,
    db: &DatabaseConnection,
    ctf_name: &str,
) -> anyhow::Result<()> {
    // Get flag configurations from sync cache
    let flag_configs = get_flag_configs(state).await;

    // If the k8s Challenge store hasn't populated yet (e.g. right after
    // startup) we'd compute zero points for everything and overwrite a
    // perfectly good DB cache.  Skip this cycle instead.
    if flag_configs.is_empty() {
        return Ok(());
    }

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
        let key = (
            ctf_name.to_string(),
            challenge_name.clone(),
            flag_name.clone(),
        );
        flag_scores.insert(key, score.clone());

        // Accumulate challenge points
        let ch_key = (ctf_name.to_string(), challenge_name.clone());
        *challenge_points.entry(ch_key).or_insert(0) += score.current_points;
    }

    // Calculate user scores & leaderboard
    let user_scores = calculate_user_scores(db, ctf_name, &flag_scores).await?;

    let mut leaderboard: Vec<UserScore> = user_scores.values().cloned().collect();
    leaderboard.sort_by(|a, b| {
        b.total_points
            .cmp(&a.total_points)
            .then_with(|| a.last_solve_at.cmp(&b.last_solve_at))
    });

    // Update in-memory cache
    let mut cache = state.scoreboard.write().await;
    cache.flag_scores = flag_scores;
    cache.challenge_points = challenge_points;
    cache.user_scores = user_scores;
    cache.leaderboards.insert(ctf_name.to_string(), leaderboard);

    // Persist to DB so a restart while frozen preserves the pre-freeze scores.
    let snapshot = cache.snapshot(ctf_name);
    drop(cache);
    if let Err(err) = save_scoreboard_cache(db, ctf_name, &snapshot).await {
        warn!(error = %err, "failed to persist scoreboard cache to database");
    }

    Ok(())
}

/// Get flag configurations from the k8s Store cache
async fn get_flag_configs(state: &AppState) -> HashMap<(String, String), (u32, u32, u32)> {
    let mut configs = HashMap::new();

    let challenges = crate::k8s::challenges::list_challenges_for_ctf(state);

    for challenge in challenges {
        for flag in &challenge.flags {
            configs.insert(
                (challenge.name.clone(), flag.name.clone()),
                (flag.base_points, flag.min_points, flag.decay),
            );
        }
    }

    configs
}

/// Get solve counts per flag from database (excludes admin submissions)
async fn get_flag_solve_counts(
    db: &DatabaseConnection,
    ctf_name: &str,
) -> anyhow::Result<HashMap<(String, String), u32>> {
    let rows: Vec<(String, String, i64)> = flag_submissions::Entity::find()
        .select_only()
        .column(flag_submissions::Column::ChallengeName)
        .column(flag_submissions::Column::FlagName)
        .column_as(Expr::cust("COUNT(DISTINCT user_id)"), "solve_count")
        .filter(flag_submissions::Column::CtfName.eq(ctf_name))
        .filter(flag_submissions::Column::Accepted.eq(true))
        .filter(flag_submissions::Column::AdminSubmission.eq(false))
        .group_by(flag_submissions::Column::ChallengeName)
        .group_by(flag_submissions::Column::FlagName)
        .into_tuple()
        .all(db)
        .await?;

    Ok(rows
        .into_iter()
        .map(|(ch, fl, cnt)| ((ch, fl), cnt as u32))
        .collect())
}

/// Calculate user scores based on their solves (excludes admin submissions)
async fn calculate_user_scores(
    db: &DatabaseConnection,
    ctf_name: &str,
    flag_scores: &HashMap<FlagKey, FlagScore>,
) -> anyhow::Result<HashMap<UserScoreKey, UserScore>> {
    // Get all accepted submissions with user info (excluding admin submissions)
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
            SELECT DISTINCT ON (s.user_id, s.challenge_name, s.flag_name)
                s.user_id,
                COALESCE(u.name, u.sub) as username,
                s.challenge_name,
                s.flag_name,
                s.created_at
            FROM flag_submissions s
            JOIN users u ON u.id = s.user_id
            WHERE s.ctf_name = $1 AND s.accepted = true AND s.admin_submission = false
            ORDER BY s.user_id, s.challenge_name, s.flag_name, s.created_at
            "#,
            [ctf_name.into()],
        ))
        .await?;

    let mut user_scores: HashMap<UserScoreKey, UserScore> = HashMap::new();

    for row in &rows {
        let user_id: i64 = row.try_get_by_index(0)?;
        let username: String = row.try_get_by_index(1)?;
        let challenge_name: String = row.try_get_by_index(2)?;
        let flag_name: String = row.try_get_by_index(3)?;
        let solved_at: chrono::DateTime<chrono::Utc> = row.try_get_by_index(4)?;
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
