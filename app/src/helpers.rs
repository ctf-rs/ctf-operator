use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};

use axum::http::HeaderMap;

use crate::db::{ChallengeView, User};

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
