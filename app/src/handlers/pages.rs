use std::{collections::{HashMap, HashSet}, sync::Arc};

use axum::{
    Extension,
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::CookieJar;
use ctf_cli::ChallengeCategory;
use kube::ResourceExt;
use sea_orm::{ConnectionTrait, DbBackend, Statement};
use serde::Deserialize;
use tracing::warn;

use crate::{
    db,
    helpers::{
        get_ctf_schedule, has_ctf_access, has_ctf_level_access, is_ctf_ended, is_ctf_open,
        is_ctf_public, seconds_until_time,
    },
    k8s::{
        challenges,
        helpers::{
            instance_expires_at_epoch, instance_phase_description, instance_ready_from_resource,
            instance_time_left_seconds, is_instance_deleting, is_instance_owned_by_user,
        },
        portclaims::get_allocated_ports_for_instance,
    },
    state::AppState,
    views,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build the display label for the current user.
///
/// Shows `"DisplayName [TeamName]"` in team mode, or just `"DisplayName"` in
/// solo mode.
fn build_user_label(user: &db::User) -> String {
    let name = user
        .name
        .clone()
        .or(user.email.clone())
        .unwrap_or_else(|| user.sub.clone());

    match user.team_name.as_deref() {
        Some(team) if !team.is_empty() => format!("{name} [{team}]"),
        _ => name,
    }
}

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct ChallengePageQuery {
    submit: Option<String>,
    flag: Option<String>,
    message: Option<String>,
    extend: Option<String>,
}

// ---------------------------------------------------------------------------
// Index (challenge list)
// ---------------------------------------------------------------------------

pub async fn index(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    // Resolve user from session cookie (index is public, so user may not exist)
    let user = match jar.get(&state.config.session_cookie_name) {
        Some(cookie) => match db::get_user_by_session(&state.db, cookie.value()).await {
            Ok(Some(mut u)) => {
                if !state.config.mode.is_team() {
                    u.team_id = None;
                    u.team_name = None;
                }
                Some(u)
            }
            Ok(None) => None,
            Err(err) => {
                warn!(error = %err, "failed to read current user");
                None
            }
        },
        None => None,
    };

    let ctf_title = crate::helpers::get_ctf_full_name(&state);

    let Some(user) = user else {
        let html = views::render_to_html(move || {
            leptos::prelude::view! { <views::LoginPage ctf_title=ctf_title/> }
        });
        return Html(html).into_response();
    };

    // Gate on non-public CTF access
    let ctf_public = is_ctf_public(&state);
    if !has_ctf_level_access(&user, ctf_public, &state.config) {
        let user_label = build_user_label(&user);
        let html = views::render_to_html(move || {
            leptos::prelude::view! {
                <views::AccessDeniedPage ctf_title=ctf_title user_label=user_label/>
            }
        });
        return Html(html).into_response();
    }

    // Gate on team membership in team mode
    if state.config.mode.is_team() && user.team_id.is_none() {
        let user_label = build_user_label(&user);
        let html = views::render_to_html(move || {
            leptos::prelude::view! {
                <views::NoTeamPage ctf_title=ctf_title user_label=user_label/>
            }
        });
        return Html(html).into_response();
    }

    // Gate on CTF open status
    let schedule = get_ctf_schedule(&state);
    if !is_ctf_open(&schedule) {
        let seconds_until_open = schedule.open_time.as_ref().map(seconds_until_time);
        let user_label = build_user_label(&user);
        let html = views::render_to_html(move || {
            leptos::prelude::view! {
                <views::CtfNotOpenPage
                    ctf_title=ctf_title
                    user_label=user_label
                    seconds_until_open=seconds_until_open
                />
            }
        });
        return Html(html).into_response();
    }

    let challenges = challenges::list_challenges_for_ctf(&state);

    let challenges = challenges
        .into_iter()
        .filter(|challenge| has_ctf_access(&user, challenge, &state.config))
        .collect::<Vec<_>>();

    let solved_keys = db::list_solved_challenges(&state.db, user.id, user.team_id.as_deref())
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|entry| format!("{}::{}", entry.ctf_name, entry.challenge_name))
        .collect::<HashSet<_>>();

    // Get partial solve counts to detect challenges with some flags submitted
    let partial_solve_counts = db::get_partial_solve_counts(&state.db, user.id, user.team_id.as_deref())
        .await
        .unwrap_or_default();

    // Get all instances for this user from Store
    let instances =
        crate::k8s::instances::list_instances_for_user(&state, user.id, user.team_id.as_deref());

    // Build a map of (ctf_name, challenge_name) -> time_left_seconds
    let mut instance_times: std::collections::HashMap<(String, String), Option<u64>> =
        std::collections::HashMap::new();

    for inst in instances {
        let time_left = instance_time_left_seconds(&inst);
        instance_times.insert(
            (inst.spec.ctf.clone(), inst.spec.challenge.clone()),
            time_left,
        );
    }

    // Build category → cards map. Challenges with multiple categories appear in all of them.
    let mut category_map: std::collections::BTreeMap<
        ChallengeCategory,
        Vec<views::ChallengeCardData>,
    > = std::collections::BTreeMap::new();

    for challenge in challenges {
        let key = format!("{}::{}", challenge.ctf_name, challenge.name);
        let solved = solved_keys.contains(&key);
        let total_flags = challenge.flags.len();

        // Get solved flags count
        let solved_flags = if solved {
            total_flags
        } else {
            partial_solve_counts
                .get(&(challenge.ctf_name.clone(), challenge.name.clone()))
                .copied()
                .unwrap_or(0) as usize
        };

        // Partial solve: some flags submitted but not fully solved
        let partial_solve = !solved && solved_flags > 0;

        // Check for instance
        let instance_key = (challenge.ctf_name.clone(), challenge.name.clone());
        let (has_instance, time_left_seconds) =
            if let Some(time_left) = instance_times.get(&instance_key) {
                (true, *time_left)
            } else {
                (false, None)
            };

        let cats: Vec<ChallengeCategory> = if challenge.categories.is_empty() {
            vec![ChallengeCategory::Misc]
        } else {
            challenge.categories.clone()
        };

        // Get points from scoreboard
        let points = {
            let scoreboard = state.scoreboard.read().await;
            scoreboard.get_challenge_points(&challenge.ctf_name, &challenge.name)
        };

        for cat in cats {
            category_map
                .entry(cat)
                .or_default()
                .push(views::ChallengeCardData {
                    challenge: challenge.clone(),
                    solved,
                    partial_solve,
                    solved_flags,
                    total_flags,
                    time_left_seconds,
                    has_instance,
                    points,
                });
        }
    }

    // Sort cards within each category by easiest flag difficulty (veryeasy → insane).
    // Challenges with no flagged difficulties are pushed to the end.
    for cards in category_map.values_mut() {
        cards.sort_by_key(|card| {
            card.challenge
                .flags
                .iter()
                .filter_map(|f| f.difficulty)
                .map(|d| d.sort_key())
                .min()
                .unwrap_or(u8::MAX)
        });
    }

    // Build sorted category sections
    let mut sections: Vec<views::CategorySection> = category_map
        .into_iter()
        .map(|(cat, cards)| views::CategorySection {
            label: cat.label().to_string(),
            category: cat,
            cards,
        })
        .collect();
    sections.sort_by_key(|s| s.category.sort_key());

    let user_label = build_user_label(&user);

    let ctf_title = crate::helpers::get_ctf_full_name(&state);
    let html = views::render_to_html(move || {
        leptos::prelude::view! {
            <views::IndexPage ctf_title=ctf_title user_label=user_label sections=sections/>
        }
    });

    Html(html).into_response()
}

// ---------------------------------------------------------------------------
// Challenge detail page
// ---------------------------------------------------------------------------

pub async fn challenge_page(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<db::User>,
    headers: axum::http::HeaderMap,
    Path(challenge_name): Path<String>,
    Query(query): Query<ChallengePageQuery>,
) -> Response {
    // Extract hostname (without port) from Host header for connection details
    let request_hostname = headers
        .get(axum::http::header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(|h| h.split(':').next().unwrap_or(h))
        .unwrap_or("localhost")
        .to_string();

    let ctf_name = &state.config.ctf_name;

    let Some(challenge) = challenges::get_challenge(&state, &challenge_name) else {
        return (axum::http::StatusCode::NOT_FOUND, "challenge not found").into_response();
    };

    if !has_ctf_access(&user, &challenge, &state.config) {
        return (axum::http::StatusCode::FORBIDDEN, "forbidden").into_response();
    }

    let instancing = challenge.instancing;

    // Mark which flags are already solved by this user/team
    let solved_flags = db::get_solved_flags(&state.db, user.id, user.team_id.as_deref(), &challenge_name, ctf_name)
        .await
        .unwrap_or_default();
    let mut challenge = challenge;
    for flag in &mut challenge.flags {
        flag.solved = solved_flags.contains(&flag.name);
    }

    let mut solved =
        db::is_challenge_solved(&state.db, user.id, user.team_id.as_deref(), &challenge_name, ctf_name)
            .await
            .unwrap_or(false);

    // Self-healing: if all flags are individually solved but the challenge
    // wasn't marked as solved (e.g. mark_challenge_solved failed earlier),
    // recover by marking it now.
    if !solved && !challenge.flags.is_empty() && challenge.flags.iter().all(|f| f.solved) {
        warn!(
            challenge = %challenge_name,
            user_id = user.id,
            "all flags solved but challenge not marked — recovering"
        );
        if let Err(err) =
            db::mark_challenge_solved(&state.db, user.id, &challenge_name, ctf_name).await
        {
            warn!(error = %err, "recovery mark_challenge_solved failed");
        }
        solved = true;
    }

    let instance = crate::k8s::instances::get_instance_for_challenge(
        &state,
        user.id,
        user.team_id.as_deref(),
        &challenge_name,
        ctf_name,
    );

    // Challenges without containers don't need instances — flags are always
    // submittable (the form should never be disabled for lack of readiness).
    let mut ready = !challenge.has_containers;
    let mut time_left_seconds: Option<u64> = None;
    let mut expires_at_epoch: Option<u64> = None;
    let mut can_extend = false;
    let mut deleting = false;
    let mut instance_phase_desc: Option<String> = None;
    let mut has_instance = false;
    let mut instance_name_str: Option<String> = None;

    if let Some(ref existing) = instance {
        if is_instance_owned_by_user(existing, &user) {
            has_instance = true;
            instance_name_str = Some(existing.name_any());
            ready = instance_ready_from_resource(existing);
            time_left_seconds = instance_time_left_seconds(existing);
            expires_at_epoch = instance_expires_at_epoch(existing);
            deleting = is_instance_deleting(existing);
            can_extend = ready
                && !deleting
                && time_left_seconds
                    .map(|seconds| seconds <= 300 && seconds > 0)
                    .unwrap_or(false);
            instance_phase_desc = Some(if deleting {
                "Stopping...".to_string()
            } else {
                instance_phase_description(existing)
            });

            // Fetch allocated ports from PortClaims when instance is ready
            if ready {
                let target_ns = existing.name_any();
                if let Ok(allocated_ports) =
                    get_allocated_ports_for_instance(&state.kube, &target_ns, &existing.name_any())
                        .await
                {
                    for (port_name, allocated) in &allocated_ports {
                        for port in &mut challenge.exposed_ports {
                            if &port.name == port_name {
                                port.allocated_port = Some(*allocated);
                            }
                        }
                    }
                }
            }
        }
    }

    let user_label = build_user_label(&user);

    let submit_feedback = views::parse_submission_feedback(
        query.submit.as_deref(),
        query.flag.as_deref(),
        query.message.as_deref(),
    );
    let extend_feedback =
        views::parse_extend_feedback(query.extend.as_deref(), query.message.as_deref());

    // Standalone error message: ?message=... without submit/extend context
    let error_message = if query.submit.is_none() && query.extend.is_none() {
        query.message.clone()
    } else {
        None
    };

    // Use config override if set, otherwise use the hostname from the request
    let instance_base_domain = state
        .config
        .instance_base_domain
        .clone()
        .unwrap_or(request_hostname);

    // Get points from scoreboard
    let points = {
        let scoreboard = state.scoreboard.read().await;
        scoreboard.get_challenge_points(&ctf_name, &challenge_name)
    };

    // Per-flag points (current dynamic points from scoreboard cache)
    let flag_points = {
        let scoreboard = state.scoreboard.read().await;
        challenge
            .flags
            .iter()
            .map(|f| scoreboard.get_flag_points(&ctf_name, &challenge_name, &f.name))
            .collect::<Vec<u32>>()
    };

    // Check if CTF has ended (disable flag submission)
    let schedule = get_ctf_schedule(&state);
    let ctf_ended = is_ctf_ended(&schedule);

    let ctf_title = crate::helpers::get_ctf_full_name(&state);

    let page_data = views::ChallengePageData {
        challenge,
        ctf_title,
        user_label,
        instancing,
        has_instance,
        ready,
        time_left_seconds,
        expires_at_epoch,
        can_extend,
        solved,
        deleting,
        submit_feedback,
        extend_feedback,
        instance_name: instance_name_str,
        instance_base_domain,
        instance_phase_desc,
        points,
        flag_points,
        ctf_ended,
        error_message,
    };

    let html = views::render_to_html(move || {
        leptos::prelude::view! { <views::ChallengeDetailPage data=page_data/> }
    });

    Html(html).into_response()
}

// ---------------------------------------------------------------------------
// Scoreboard
// ---------------------------------------------------------------------------

pub async fn scoreboard(
    State(state): State<Arc<AppState>>,
    user_ext: Option<Extension<db::User>>,
) -> Response {
    let user = user_ext.map(|Extension(u)| u);
    let sb_config = crate::helpers::get_ctf_scoreboard_config(&state);

    // Anonymous visitor on a non-public scoreboard → redirect to login
    if user.is_none() && !sb_config.public {
        return Redirect::to("/login").into_response();
    }

    let ctf_name = state.config.ctf_name.clone();
    let ctf_title = crate::helpers::get_ctf_full_name(&state);

    // Gate on non-public CTF access (skip for anonymous visitors on public scoreboard)
    if let Some(ref user) = user {
        let ctf_public = is_ctf_public(&state);
        if !has_ctf_level_access(user, ctf_public, &state.config) {
            let user_label = build_user_label(user);
            let html = views::render_to_html(move || {
                leptos::prelude::view! {
                    <views::AccessDeniedPage ctf_title=ctf_title user_label=user_label/>
                }
            });
            return Html(html).into_response();
        }
    }

    // Gate on CTF open status (skip for anonymous visitors on public scoreboard)
    if let Some(ref user) = user {
        let schedule = get_ctf_schedule(&state);
        if !is_ctf_open(&schedule) {
            let seconds_until_open = schedule.open_time.as_ref().map(seconds_until_time);
            let user_label = build_user_label(user);
            let html = views::render_to_html(move || {
                leptos::prelude::view! {
                    <views::CtfNotOpenPage
                        ctf_title=ctf_title
                        user_label=user_label
                        seconds_until_open=seconds_until_open
                    />
                }
            });
            return Html(html).into_response();
        }
    }

    // Gate on scoreboard visibility
    if !sb_config.visible {
        let user_label = user.as_ref().map(|u| build_user_label(u)).unwrap_or_default();
        let html = views::render_to_html(move || {
            leptos::prelude::view! {
                <views::ScoreboardHiddenPage ctf_title=ctf_title user_label=user_label/>
            }
        });
        return Html(html).into_response();
    }

    let user_label = user.as_ref().map(|u| build_user_label(u)).unwrap_or_default();

    // Get leaderboard + challenge points from scoreboard cache in one read
    let (leaderboard, challenge_points_cache) = {
        let scoreboard = state.scoreboard.read().await;
        let mut lb = scoreboard.get_leaderboard(&ctf_name);
        let cp = scoreboard.challenge_points.clone();
        // In team mode, only show teams on the scoreboard
        if state.config.mode.is_team() {
            lb.retain(|e| e.team_id.is_some());
        }
        (lb, cp)
    };

    let frozen = crate::helpers::is_scoreboard_frozen(&sb_config);

    // Build challenge columns (sorted by category, then difficulty)
    let all_challenges = challenges::list_challenges_for_ctf(&state);
    let mut challenge_columns: Vec<views::ScoreboardChallenge> = all_challenges
        .iter()
        .map(|ch| {
            // Pick the hardest difficulty from all flags, or None
            let difficulty = ch
                .flags
                .iter()
                .filter_map(|f| f.difficulty)
                .max_by_key(|d| d.sort_key());
            let category = ch.categories.first().copied().unwrap_or(ChallengeCategory::Misc);
            let points = challenge_points_cache
                .get(&(ctf_name.clone(), ch.name.clone()))
                .copied()
                .unwrap_or(0);
            views::ScoreboardChallenge {
                name: ch.name.clone(),
                full_name: ch.full_name.clone(),
                category,
                difficulty,
                points,
            }
        })
        .collect();
    challenge_columns.sort_by(|a, b| {
        a.category
            .sort_key()
            .cmp(&b.category.sort_key())
            .then_with(|| {
                a.difficulty
                    .map(|d| d.sort_key())
                    .unwrap_or(255)
                    .cmp(&b.difficulty.map(|d| d.sort_key()).unwrap_or(255))
            })
            .then_with(|| a.name.cmp(&b.name))
    });

    // Gather per-challenge solve data: who solved what and first blood
    let solve_data = get_scoreboard_solve_data(&state.db, &ctf_name).await;
    let first_blood = compute_first_blood(&solve_data);

    // Build the solve grid: one row per leaderboard entry, one cell per challenge column
    let solve_grid: Vec<Vec<views::SolveStatus>> = leaderboard
        .iter()
        .map(|entry| {
            challenge_columns
                .iter()
                .map(|ch| {
                    let is_first = first_blood
                        .get(&ch.name)
                        .is_some_and(|fb_id| *fb_id == entry.entity_id);
                    let is_solved = solve_data
                        .get(&(entry.entity_id.clone(), ch.name.clone()))
                        .is_some();
                    if is_first {
                        views::SolveStatus::FirstBlood
                    } else if is_solved {
                        views::SolveStatus::Solved
                    } else {
                        views::SolveStatus::Unsolved
                    }
                })
                .collect()
        })
        .collect();

    let has_teams = state.config.mode.is_team();

    let page_data = views::ScoreboardPageData {
        ctf_title,
        user_label,
        leaderboard,
        frozen,
        challenge_columns,
        solve_grid,
        has_teams,
    };

    let html = views::render_to_html(move || {
        leptos::prelude::view! { <views::ScoreboardPage data=page_data/> }
    });

    Html(html).into_response()
}

// ---------------------------------------------------------------------------
// Scoreboard helpers — solve grid + first blood
// ---------------------------------------------------------------------------

/// Per-challenge solve data: `(entity_id, challenge_name) -> earliest_solve_timestamp`.
///
/// `entity_id` = `team_id` when the user is on a team, otherwise the
/// stringified `user_id`.  This matches the scoring entity used in the
/// leaderboard so the solve grid aligns.
async fn get_scoreboard_solve_data(
    db: &sea_orm::DatabaseConnection,
    ctf_name: &str,
) -> HashMap<(String, String), chrono::DateTime<chrono::Utc>> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
            SELECT COALESCE(u.team_id, s.user_id::text) AS entity_id,
                   s.challenge_name,
                   MIN(s.created_at) AS first_solve
            FROM flag_submissions s
            JOIN users u ON u.id = s.user_id
            WHERE s.ctf_name = $1 AND s.accepted = true AND s.admin_submission = false
            GROUP BY COALESCE(u.team_id, s.user_id::text), s.challenge_name
            "#,
            [ctf_name.into()],
        ))
        .await;

    let Ok(rows) = rows else {
        return HashMap::new();
    };

    let mut result = HashMap::new();
    for row in &rows {
        let entity_id: String = row.try_get_by_index(0).unwrap_or_default();
        let challenge: String = row.try_get_by_index(1).unwrap_or_default();
        let ts: chrono::DateTime<chrono::Utc> = match row.try_get_by_index(2) {
            Ok(t) => t,
            Err(_) => continue,
        };
        result.insert((entity_id, challenge), ts);
    }
    result
}

/// Compute first blood: `challenge_name -> entity_id` of the first solver.
fn compute_first_blood(
    solve_data: &HashMap<(String, String), chrono::DateTime<chrono::Utc>>,
) -> HashMap<String, String> {
    let mut first: HashMap<String, (String, chrono::DateTime<chrono::Utc>)> = HashMap::new();
    for ((entity_id, challenge), ts) in solve_data {
        first
            .entry(challenge.clone())
            .and_modify(|(fb_eid, fb_ts)| {
                if ts < fb_ts {
                    fb_eid.clone_from(entity_id);
                    *fb_ts = *ts;
                }
            })
            .or_insert((entity_id.clone(), *ts));
    }
    first
        .into_iter()
        .map(|(ch, (eid, _))| (ch, eid))
        .collect()
}
