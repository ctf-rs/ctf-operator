use std::sync::Arc;

use axum::{
    extract::{Query, State},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::extract::cookie::{Cookie, CookieJar};
use serde::Deserialize;
use tracing::warn;
use uuid::Uuid;

use crate::{db, oauth, state::AppState};

// ---------------------------------------------------------------------------
// Request types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct OauthCallbackQuery {
    code: String,
    state: String,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

pub async fn healthz() -> impl IntoResponse {
    "ok"
}

pub async fn login(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let Ok((auth_url, state_token, nonce)) = oauth::begin_login(&state.oidc_client) else {
        return Redirect::to("/");
    };

    state
        .pending_oauth
        .write()
        .await
        .insert(state_token, oauth::PendingAuth { nonce });

    Redirect::to(auth_url.as_str())
}

pub async fn oauth_callback(
    State(state): State<Arc<AppState>>,
    jar: CookieJar,
    Query(query): Query<OauthCallbackQuery>,
) -> Response {
    let pending = state.pending_oauth.write().await.remove(&query.state);
    let Some(_pending) = pending else {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid oauth state").into_response();
    };

    let claims = match oauth::finish_login(&state.oidc_client, query.code).await {
        Ok(claims) => claims,
        Err(err) => {
            warn!(error = %err, "oauth callback failed");
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                "oauth callback failed",
            )
                .into_response();
        }
    };

    let team_id = if state.config.mode.is_team() {
        claims.team_id.as_deref()
    } else {
        None
    };

    let user = match db::upsert_user(
        &state.db,
        &claims.sub,
        claims.email.as_deref(),
        claims.name.as_deref(),
        team_id,
    )
    .await
    {
        Ok(user) => user,
        Err(err) => {
            warn!(error = %err, "failed to upsert user");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "failed to persist user",
            )
                .into_response();
        }
    };

    let token = Uuid::new_v4().to_string();
    if let Err(err) = db::create_session(&state.db, &token, user.id, &claims.roles).await {
        warn!(error = %err, "failed to create session");
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "failed to create session",
        )
            .into_response();
    }

    let cookie = Cookie::build((state.config.session_cookie_name.clone(), token))
        .path("/")
        .http_only(true)
        .secure(false)
        .build();

    (jar.add(cookie), Redirect::to("/")).into_response()
}

pub async fn logout(State(state): State<Arc<AppState>>, jar: CookieJar) -> Response {
    if let Some(cookie) = jar.get(&state.config.session_cookie_name) {
        let _ = db::delete_session(&state.db, cookie.value()).await;
    }

    let cookie = Cookie::build((state.config.session_cookie_name.clone(), String::new()))
        .path("/")
        .build();

    (jar.remove(cookie), Redirect::to("/")).into_response()
}
