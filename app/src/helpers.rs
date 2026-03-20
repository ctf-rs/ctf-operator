use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::HeaderMap;
use kube::ResourceExt;

use crate::config::AppConfig;
use crate::db::{ChallengeView, User};
use crate::state::AppState;

/// Build the canonical URL for a challenge page.
pub fn challenge_route(challenge_name: &str) -> String {
    format!("/challenge/{}/", urlencoding::encode(challenge_name))
}

/// Sanitise an arbitrary string so it is valid as part of a Kubernetes
/// resource name.
pub fn sanitize_k8s_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "challenge".to_string()
    } else {
        out
    }
}

/// Current UNIX timestamp in seconds.
pub fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Whether the CTF is public (visible to all authenticated users).
pub fn is_ctf_public(state: &AppState) -> bool {
    state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == state.config.ctf_name)
        .map(|c| c.spec.public)
        .unwrap_or(false)
}

/// Check whether `user` is allowed to access the CTF at all.
///
/// Access is granted when:
/// - the CTF is public, **or**
/// - the user holds the configured admin role, **or**
/// - the user holds the configured player role.
pub fn has_ctf_level_access(user: &User, ctf_public: bool, config: &AppConfig) -> bool {
    if ctf_public {
        return true;
    }
    if is_admin(user, config) {
        return true;
    }
    if let Some(ref player_role) = config.player_role {
        return user
            .roles
            .iter()
            .any(|r| r.eq_ignore_ascii_case(player_role));
    }
    false
}

/// Check whether `user` is allowed to see `challenge` based on the CTF's
/// public flag and the user's role list.
///
/// Access is granted when:
/// - the CTF is public, **or**
/// - the user holds the configured admin role, **or**
/// - the user holds the configured player role.
pub fn has_ctf_access(user: &User, challenge: &ChallengeView, config: &AppConfig) -> bool {
    has_ctf_level_access(user, challenge.ctf_public, config)
}

/// Whether the user holds the configured admin role.
pub fn is_admin(user: &User, config: &AppConfig) -> bool {
    if let Some(ref admin_role) = config.admin_role {
        return user
            .roles
            .iter()
            .any(|r| r.eq_ignore_ascii_case(admin_role));
    }
    false
}

/// Extract client IP address based on configuration.
/// If behind a reverse proxy, checks X-Forwarded-For, X-Real-IP, CF-Connecting-IP headers.
/// Otherwise, uses the socket connection IP directly.
pub fn extract_client_ip(
    behind_reverse_proxy: bool,
    headers: &HeaderMap,
    socket_addr: Option<SocketAddr>,
) -> Option<String> {
    if behind_reverse_proxy {
        // Check X-Forwarded-For first (most common proxy header)
        if let Some(forwarded_for) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
            // X-Forwarded-For can contain multiple IPs; take the first (original client)
            if let Some(first_ip) = forwarded_for.split(',').next() {
                let ip = first_ip.trim();
                if !ip.is_empty() {
                    return Some(ip.to_string());
                }
            }
        }

        // Check X-Real-IP (nginx commonly uses this)
        if let Some(real_ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()) {
            let ip = real_ip.trim();
            if !ip.is_empty() {
                return Some(ip.to_string());
            }
        }

        // Check CF-Connecting-IP (Cloudflare)
        if let Some(cf_ip) = headers
            .get("cf-connecting-ip")
            .and_then(|v| v.to_str().ok())
        {
            let ip = cf_ip.trim();
            if !ip.is_empty() {
                return Some(ip.to_string());
            }
        }

        // Fallback to socket address if no proxy headers found
        socket_addr.map(|addr| addr.ip().to_string())
    } else {
        // Direct connection - use socket IP
        socket_addr.map(|addr| addr.ip().to_string())
    }
}

// ---------------------------------------------------------------------------
// CTF schedule / scoreboard helpers
// ---------------------------------------------------------------------------

/// Check whether the CTF resource exists in the k8s Store.
pub fn ctf_exists(state: &AppState) -> bool {
    state
        .ctfs
        .state()
        .iter()
        .any(|c| c.name_any() == state.config.ctf_name)
}

/// Get the CTF resource's schedule from the k8s Store.
pub fn get_ctf_schedule(state: &AppState) -> ctf_cli::models::CtfSchedule {
    state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == state.config.ctf_name)
        .map(|c| c.spec.schedule.clone())
        .unwrap_or_default()
}

/// Get the CTF resource's full name from the k8s Store.
///
/// Falls back to the CRD name if the resource is missing.
pub fn get_ctf_full_name(state: &AppState) -> String {
    state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == state.config.ctf_name)
        .map(|c| c.spec.full_name.clone())
        .unwrap_or_else(|| state.config.ctf_name.clone())
}

/// Get the CTF resource's scoreboard config from the k8s Store.
pub fn get_ctf_scoreboard_config(state: &AppState) -> ctf_cli::models::CtfScoreboard {
    state
        .ctfs
        .state()
        .into_iter()
        .find(|c| c.name_any() == state.config.ctf_name)
        .map(|c| c.spec.scoreboard.clone())
        .unwrap_or_default()
}

/// Whether the CTF is currently open for participants.
///
/// Open when: `schedule.open == Some(true)` **or**
///            `open_time` is set and now ≥ `open_time`.
/// When neither `open` nor `open_time` is set the CTF is considered open
/// (backwards-compatible default).
pub fn is_ctf_open(schedule: &ctf_cli::models::CtfSchedule) -> bool {
    if schedule.open == Some(true) {
        return true;
    }
    if schedule.open == Some(false) {
        // Explicitly closed — check if open_time has arrived
        if let Some(ref t) = schedule.open_time {
            return now_unix_seconds() >= t.0.as_second() as u64;
        }
        return false;
    }
    // open == None — check open_time if set, otherwise open by default
    if let Some(ref t) = schedule.open_time {
        return now_unix_seconds() >= t.0.as_second() as u64;
    }
    true
}

/// Whether the CTF has ended.
///
/// Ended when: `schedule.ended == Some(true)` **or**
///             `end_time` is set and now ≥ `end_time`.
pub fn is_ctf_ended(schedule: &ctf_cli::models::CtfSchedule) -> bool {
    if schedule.ended == Some(true) {
        return true;
    }
    if let Some(ref t) = schedule.end_time {
        return now_unix_seconds() >= t.0.as_second() as u64;
    }
    false
}

/// Seconds until a `Time` value from now. Returns 0 if already past.
pub fn seconds_until_time(t: &k8s_openapi::apimachinery::pkg::apis::meta::v1::Time) -> u64 {
    let target = t.0.as_second() as u64;
    let now = now_unix_seconds();
    target.saturating_sub(now)
}

/// Whether the scoreboard is frozen (either explicitly or by freeze time).
pub fn is_scoreboard_frozen(sb: &ctf_cli::models::CtfScoreboard) -> bool {
    if sb.frozen == Some(true) {
        return true;
    }
    if let Some(ref t) = sb.freeze_time {
        return now_unix_seconds() >= t.0.as_second() as u64;
    }
    false
}
