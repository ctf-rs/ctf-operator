use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tracing::warn;

use crate::{db, middleware::extract_session_cookie, state::AppState};

type HmacSha256 = Hmac<Sha256>;

/// Maximum age (in seconds) for a discord link token before it's considered
/// expired.  5 minutes should be plenty for a user to click the link.
const TOKEN_MAX_AGE_SECS: i64 = 300;

// ---------------------------------------------------------------------------
// Query parameters
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct DiscordLinkQuery {
    /// Base64-encoded JSON payload: `{"discord_id":"...","discord_username":"...","ts":...}`
    data: String,
    /// Hex-encoded HMAC-SHA256 signature of the raw `data` string.
    sig: String,
}

#[derive(Deserialize)]
struct DiscordLinkPayload {
    discord_id: String,
    discord_username: String,
    /// Unix timestamp (seconds) when the token was created.
    ts: i64,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /discord?data=<base64>&sig=<hex>`
///
/// The Discord bot creates a token by:
/// 1. Building a JSON payload `{"discord_id":"...","discord_username":"...","ts":<unix_secs>}`.
/// 2. Base64-encoding the JSON → this is `data`.
/// 3. Computing `HMAC-SHA256(secret, data)` → hex-encoding → this is `sig`.
///
/// The user must be **logged in** (have a valid session cookie).  On success
/// the handler stores the discord id/username on the user row and renders a
/// success page.
pub async fn discord_link(
    State(state): State<Arc<AppState>>,
    headers: axum::http::HeaderMap,
    Query(query): Query<DiscordLinkQuery>,
) -> Response {
    // 1. Ensure the feature is enabled.
    let secret = match state.config.discord_link_secret.as_deref() {
        Some(s) => s,
        None => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                "discord linking is not enabled",
            )
                .into_response();
        }
    };

    // 2. Authenticate the user from the session cookie.
    let token = match extract_session_cookie(&headers, &state.config.session_cookie_name) {
        Some(t) => t,
        None => {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                Html(
                    r#"<html><body style="background:#111;color:#e5e7eb;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center"><h1>Not Logged In</h1><p>Please <a href="/login" style="color:#60a5fa">log in</a> first, then open this link again.</p></div></body></html>"#
                ),
            )
                .into_response();
        }
    };

    let user = match db::get_user_by_session(&state.db, &token).await {
        Ok(Some(u)) => u,
        _ => {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                Html(
                    r#"<html><body style="background:#111;color:#e5e7eb;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0"><div style="text-align:center"><h1>Session Expired</h1><p>Please <a href="/login" style="color:#60a5fa">log in</a> again, then re-open this link.</p></div></body></html>"#
                ),
            )
                .into_response();
        }
    };

    // 3. Verify the HMAC signature.
    let sig_bytes = match hex::decode(&query.sig) {
        Ok(b) => b,
        Err(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                "invalid signature encoding",
            )
                .into_response();
        }
    };

    let mut mac = match HmacSha256::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => {
            warn!("discord link secret is invalid for HMAC");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "server config error",
            )
                .into_response();
        }
    };
    mac.update(query.data.as_bytes());
    if mac.verify_slice(&sig_bytes).is_err() {
        return (axum::http::StatusCode::FORBIDDEN, "invalid signature").into_response();
    }

    // 4. Decode the payload.
    let json_bytes = match base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &query.data,
    ) {
        Ok(b) => b,
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid data encoding").into_response();
        }
    };

    let payload: DiscordLinkPayload = match serde_json::from_slice(&json_bytes) {
        Ok(p) => p,
        Err(_) => {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid payload").into_response();
        }
    };

    // 5. Check freshness.
    let now = chrono::Utc::now().timestamp();
    if (now - payload.ts).abs() > TOKEN_MAX_AGE_SECS {
        return (
            axum::http::StatusCode::GONE,
            "link expired – ask the bot for a new one",
        )
            .into_response();
    }

    // 6. Link Discord to the user.
    if let Err(err) = db::link_discord(
        &state.db,
        user.id,
        &payload.discord_id,
        &payload.discord_username,
    )
    .await
    {
        warn!(error = %err, user_id = user.id, "failed to link discord");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save discord link",
        )
            .into_response();
    }

    tracing::info!(
        user_id = user.id,
        discord_id = %payload.discord_id,
        discord_username = %payload.discord_username,
        "discord account linked"
    );

    // 7. Notify the Discord bot so it can assign the competitor role.
    if let Some(ref callback_url) = state.config.discord_bot_callback_url {
        notify_bot_callback(callback_url, secret, &payload.discord_id).await;
    }

    // 8. Render success page.
    let html = format!(
        r#"<html><body style="background:#111;color:#e5e7eb;font-family:sans-serif;display:flex;align-items:center;justify-content:center;height:100vh;margin:0">
<div style="text-align:center">
<h1 style="color:#34d399">&#x2714; Discord Linked!</h1>
<p>Your Discord account <strong>{}</strong> (<code>{}</code>) has been linked to your CTF account.</p>
<p style="margin-top:1.5rem"><a href="/" style="color:#60a5fa">Back to Challenges</a></p>
</div></body></html>"#,
        html_escape(&payload.discord_username),
        html_escape(&payload.discord_id),
    );

    Html(html).into_response()
}

/// Minimal HTML escaping for user-supplied strings rendered in the success page.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Fire-and-forget POST to the Discord bot's callback endpoint so it can
/// assign the competitor role.  Errors are logged but never block the user.
async fn notify_bot_callback(callback_url: &str, secret: &str, discord_id: &str) {
    let ts = chrono::Utc::now().timestamp();
    let payload = serde_json::json!({ "discord_id": discord_id, "ts": ts });
    let json_bytes = serde_json::to_vec(&payload).expect("json");
    let data = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        &json_bytes,
    );

    let mut mac = match <Hmac<Sha256>>::new_from_slice(secret.as_bytes()) {
        Ok(m) => m,
        Err(_) => return,
    };
    mac.update(data.as_bytes());
    let sig = hex::encode(mac.finalize().into_bytes());

    let url = format!("{}/verified", callback_url.trim_end_matches('/'));
    let body = serde_json::json!({ "data": data, "sig": sig });

    let client = reqwest::Client::new();
    match client.post(&url).json(&body).send().await {
        Ok(resp) if resp.status().is_success() => {
            tracing::info!(discord_id, "bot callback succeeded");
        }
        Ok(resp) => {
            tracing::warn!(
                discord_id,
                status = %resp.status(),
                "bot callback returned non-success"
            );
        }
        Err(e) => {
            tracing::warn!(discord_id, error = %e, "bot callback request failed");
        }
    }
}
