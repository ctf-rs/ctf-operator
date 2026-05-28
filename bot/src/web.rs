//! Tiny HTTP server that receives callbacks from the CTF web application.
//!
//! After a user successfully links their Discord account on the website, the
//! web app POSTs a signed payload here so the bot can assign the competitor
//! role without the user having to do anything else.

use std::sync::Arc;

use axum::{Json, Router, extract::State, response::IntoResponse, routing::post};
use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use serde::Deserialize;
use sha2::Sha256;
use tracing::{error, info, warn};

use crate::config::BotConfig;
use crate::handler::assign_competitor_role;

type HmacSha256 = Hmac<Sha256>;

/// Maximum accepted age (seconds) for a callback token.
const CALLBACK_MAX_AGE_SECS: u64 = 300;

// ---------------------------------------------------------------------------
// Shared state for axum handlers
// ---------------------------------------------------------------------------

pub struct WebState {
    pub config: Arc<BotConfig>,
    pub http: Arc<serenity::http::Http>,
}

// ---------------------------------------------------------------------------
// Router
// ---------------------------------------------------------------------------

pub fn router(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/verified", post(handle_verified))
        .with_state(state)
}

// ---------------------------------------------------------------------------
// POST /verified
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct VerifiedBody {
    /// Base64url-encoded JSON: `{"discord_id":"...","ts":...}`
    data: String,
    /// Hex-encoded HMAC-SHA256 of `data`.
    sig: String,
}

#[derive(Deserialize)]
struct VerifiedPayload {
    discord_id: String,
    ts: u64,
}

async fn handle_verified(
    State(state): State<Arc<WebState>>,
    Json(body): Json<VerifiedBody>,
) -> impl IntoResponse {
    // 1. Verify HMAC.
    let Ok(sig_bytes) = hex::decode(&body.sig) else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid sig encoding");
    };

    let Ok(mut mac) = HmacSha256::new_from_slice(state.config.discord_link_secret.as_bytes())
    else {
        error!("DISCORD_LINK_SECRET is invalid for HMAC");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "server error",
        );
    };
    mac.update(body.data.as_bytes());
    if mac.verify_slice(&sig_bytes).is_err() {
        return (axum::http::StatusCode::FORBIDDEN, "bad signature");
    }

    // 2. Decode payload.
    let Ok(json_bytes) = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&body.data) else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid data encoding");
    };

    let Ok(payload) = serde_json::from_slice::<VerifiedPayload>(&json_bytes) else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid payload");
    };

    // 3. Freshness check.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_secs();
    if now.abs_diff(payload.ts) > CALLBACK_MAX_AGE_SECS {
        return (axum::http::StatusCode::GONE, "expired");
    }

    // 4. Parse discord id.
    let Ok(discord_uid) = payload.discord_id.parse::<u64>() else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid discord_id");
    };

    // 5. Assign the competitor role.
    if let Err(e) = assign_competitor_role(
        &state.http,
        state.config.guild_id,
        discord_uid,
        &state.config.competitor_role_name,
    )
    .await
    {
        warn!(error = %e, discord_id = %payload.discord_id, "role assignment failed");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "role assignment failed",
        );
    }

    info!(discord_id = %payload.discord_id, "verified callback processed");
    (axum::http::StatusCode::OK, "ok")
}
