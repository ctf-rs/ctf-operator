use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use axum_extra::extract::cookie::CookieJar;

use crate::{
    db::{self, ChallengeView, User},
    state::AppState,
};

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

/// Check whether `user` is allowed to see `challenge` based on the CTF's
/// public flag and the user's role list.
pub fn has_ctf_access(user: &User, challenge: &ChallengeView) -> bool {
    if challenge.ctf_public {
        return true;
    }

    let namespace = challenge.namespace.to_ascii_lowercase();
    let ctf_name = challenge.ctf_name.to_ascii_lowercase();

    let required_roles = [
        format!("ctf:{namespace}:{ctf_name}:player"),
        format!("ctf:{namespace}:{ctf_name}:admin"),
        "ctf:admin".to_string(),
    ];

    user.roles.iter().any(|role| {
        required_roles
            .iter()
            .any(|required| role.eq_ignore_ascii_case(required))
    })
}

/// Resolve the currently-logged-in user from the session cookie, if any.
pub async fn current_user(
    state: &Arc<AppState>,
    jar: &CookieJar,
) -> anyhow::Result<Option<User>> {
    let Some(session_cookie) = jar.get(&state.config.session_cookie_name) else {
        return Ok(None);
    };

    db::get_user_by_session(&state.db, session_cookie.value()).await
}
