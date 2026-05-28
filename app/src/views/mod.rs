mod components;
mod layout;
mod pages;
mod scripts;

pub use pages::{
    AccessDeniedPage, ChallengeDetailPage, CtfNotOpenPage, IndexPage, LoginPage,
    ScoreboardHiddenPage, ScoreboardPage,
};

use ctf_cli::{ChallengeCategory, ChallengeDifficulty};
use leptos::prelude::*;

use crate::db::ChallengeView;
use crate::scoreboard::UserScore;

// ---------------------------------------------------------------------------
// Data types shared between views and handlers
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ChallengeCardData {
    pub challenge: ChallengeView,
    pub solved: bool,
    pub partial_solve: bool, // Some flags submitted but not all
    pub solved_flags: usize,
    pub total_flags: usize,
    pub time_left_seconds: Option<u64>,
    pub has_instance: bool,
    /// Total points this challenge is worth (sum of flag points)
    pub points: u32,
}

#[derive(Debug)]
pub struct CategorySection {
    pub category: ChallengeCategory,
    pub label: String,
    pub cards: Vec<ChallengeCardData>,
}

#[derive(Debug)]
pub struct SubmissionFeedback {
    pub kind: String,
    pub flag_name: String,
    pub message: String,
}

#[derive(Debug)]
pub struct ChallengePageData {
    pub challenge: ChallengeView,
    pub ctf_title: String,
    pub user_label: String,
    pub instancing: bool,
    pub has_instance: bool,
    pub ready: bool,
    pub time_left_seconds: Option<u64>,
    /// Unix epoch (seconds) when the instance timer expires.
    pub expires_at_epoch: Option<u64>,
    pub can_extend: bool,
    pub solved: bool,
    pub deleting: bool,
    pub submit_feedback: Option<SubmissionFeedback>,
    pub extend_feedback: Option<String>,
    pub instance_name: Option<String>,
    pub instance_base_domain: String,
    pub instance_phase_desc: Option<String>,
    /// Total points this challenge is worth (sum of flag points)
    pub points: u32,
    /// Current points per-flag (same order as `challenge.flags`)
    pub flag_points: Vec<u32>,
    /// Whether the CTF has ended (flag submission disabled)
    pub ctf_ended: bool,
    /// Generic error message to display (e.g. max instances reached)
    pub error_message: Option<String>,
}

#[derive(Debug)]
pub struct ScoreboardPageData {
    pub ctf_title: String,
    pub user_label: String,
    pub leaderboard: Vec<UserScore>,
    pub frozen: bool,
    /// Columns for the challenge-level grid (sorted by category then difficulty).
    pub challenge_columns: Vec<ScoreboardChallenge>,
    /// Per-entry solve details: `leaderboard[i].solved_challenges[j]` corresponds
    /// to `challenge_columns[j]`.
    pub solve_grid: Vec<Vec<SolveStatus>>,
    /// Whether any user has a team (enables "Team" column header instead of "Player").
    pub has_teams: bool,
}

/// One column in the scoreboard challenge grid.
#[derive(Debug, Clone)]
pub struct ScoreboardChallenge {
    pub name: String,
    pub full_name: String,
    pub category: ChallengeCategory,
    pub difficulty: Option<ChallengeDifficulty>,
    pub points: u32,
}

/// Solve status for a single (entry, challenge) cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SolveStatus {
    /// Not solved.
    Unsolved,
    /// Solved (but not first blood).
    Solved,
    /// First blood — this entry solved the challenge first.
    FirstBlood,
}

// ---------------------------------------------------------------------------
// Helpers (used across view sub-modules)
// ---------------------------------------------------------------------------

pub(crate) fn file_route(challenge_name: &str, file_idx: usize) -> String {
    format!("/file/{}/{}", urlencoding::encode(challenge_name), file_idx)
}

pub(crate) fn display_file_name(file_url: &str) -> String {
    if let Ok(parsed) = url::Url::parse(file_url) {
        if let Some(name) = parsed
            .path_segments()
            .and_then(|mut segments| segments.next_back())
            .filter(|name| !name.is_empty())
        {
            return name.to_string();
        }
    }
    file_url
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(file_url)
        .to_string()
}

pub(crate) fn format_countdown(seconds: u64) -> String {
    let hours = seconds / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;
    if hours > 0 {
        format!("{hours}h {minutes}m {secs}s")
    } else if minutes > 0 {
        format!("{minutes}m {secs}s")
    } else {
        format!("{secs}s")
    }
}

// ---------------------------------------------------------------------------
// Rendering entry-point
// ---------------------------------------------------------------------------

pub fn render_to_html<V: IntoView>(f: impl FnOnce() -> V) -> String {
    let owner = Owner::new();
    let html = owner.with(|| f().to_html());
    format!("<!DOCTYPE html>{html}")
}

// ---------------------------------------------------------------------------
// Feedback parsers
// ---------------------------------------------------------------------------

pub fn parse_submission_feedback(
    submit: Option<&str>,
    flag: Option<&str>,
    message: Option<&str>,
) -> Option<SubmissionFeedback> {
    let result = submit?;
    let kind = match result {
        "good" | "bad" => result,
        _ => return None,
    };
    Some(SubmissionFeedback {
        kind: kind.to_string(),
        flag_name: flag.unwrap_or("flag").to_string(),
        message: message.unwrap_or_default().to_string(),
    })
}

pub fn parse_extend_feedback(extend: Option<&str>, message: Option<&str>) -> Option<String> {
    match extend {
        Some("ok") => Some(message.unwrap_or("instance extended").to_string()),
        Some("bad") => Some(message.unwrap_or("failed to extend instance").to_string()),
        _ => None,
    }
}
