mod auth;
mod discord;
mod files;
pub mod flags;
pub mod instances;
mod pages;

use std::sync::Arc;

use axum::{
    Extension, Router,
    body::Body,
    middleware as axum_mw,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use axum_tracing_opentelemetry::middleware::{OtelAxumLayer, OtelInResponseLayer};
use http::Request;
use leptos::prelude::*;
use tower_http::services::ServeDir;

use crate::{
    db,
    middleware::{
        extract_session_cookie, http_metrics, optional_auth, require_auth, require_ctf_exists,
        require_ctf_open,
    },
    state::AppState,
};

/// Axum handler that dispatches Leptos server-function requests.
///
/// Resolves the authenticated user (if any) from the session cookie and
/// provides both `Arc<AppState>` and `db::User` to the Leptos context so
/// server functions can access them via `expect_context` / `use_context`.
async fn server_fn_handler(req: Request<Body>) -> Response {
    let state: Arc<AppState> = req
        .extensions()
        .get::<Arc<AppState>>()
        .expect("AppState extension missing — add Extension(state) layer to the router")
        .clone();

    // Block all server functions when the CTF has not opened yet.
    let schedule = crate::helpers::get_ctf_schedule(&state);
    if !crate::helpers::is_ctf_open(&schedule) {
        return (axum::http::StatusCode::FORBIDDEN, "CTF has not started yet").into_response();
    }

    // Resolve user from session cookie before dispatching to server fns.
    // `handle_server_fns_with_context` snapshots request Parts *before* any
    // `#[middleware]` runs, so extensions added by tower middleware are
    // invisible to `leptos_axum::extract()`.  We solve this by resolving the
    // user here and injecting it via `provide_context`.
    let user = resolve_user(&state, req.headers()).await;

    leptos_axum::handle_server_fns_with_context(
        move || {
            provide_context(state.clone());
            if let Some(ref user) = user {
                provide_context(user.clone());
            }
        },
        req,
    )
    .await
    .into_response()
}

/// Look up the authenticated user from the session cookie (if present).
async fn resolve_user(state: &AppState, headers: &http::HeaderMap) -> Option<db::User> {
    let token = extract_session_cookie(headers, &state.config.session_cookie_name)?;
    let mut user = db::get_user_by_session(&state.db, &token)
        .await
        .ok()
        .flatten()?;
    if !state.config.mode.is_team() {
        user.team_id = None;
        user.team_name = None;
    }
    Some(user)
}

/// Build the application router with all routes wired to their handlers.
pub fn router(state: Arc<AppState>) -> Router {
    // Protected page routes (auth checked via axum middleware)
    let protected_pages = Router::new()
        .route("/challenge/{challenge_name}/", get(pages::challenge_page))
        .route(
            "/challenge/{challenge_name}/instance-state",
            get(instances::instance_state),
        )
        .route(
            "/challenge/{challenge_name}/instance-events",
            get(instances::instance_events),
        )
        .route(
            "/file/{challenge_name}/{file_idx}",
            get(files::download_file),
        )
        .route_layer(axum_mw::from_fn_with_state(state.clone(), require_ctf_open))
        .route_layer(axum_mw::from_fn_with_state(state.clone(), require_auth));

    // Scoreboard uses optional_auth so it can serve anonymous visitors
    // when the CRD's scoreboard.public is true (checked at request time).
    let scoreboard_route = Router::new()
        .route("/scoreboard", get(pages::scoreboard))
        .route_layer(axum_mw::from_fn_with_state(
            state.clone(),
            optional_auth,
        ));

    // Public routes (no auth required)
    let public = Router::new()
        .route("/", get(pages::index))
        .route("/discord", get(discord::discord_link));

    Router::new()
        // Server functions (spawn, extend, stop, submit_flag, …)
        .route(
            "/api/{*fn_name}",
            get(server_fn_handler).post(server_fn_handler),
        )
        .merge(protected_pages)
        .merge(scoreboard_route)
        .merge(public)
        // Require CTF resource to exist for all routes above
        .route_layer(axum_mw::from_fn_with_state(
            state.clone(),
            require_ctf_exists,
        ))
        // Auth & health routes are exempt from CTF-existence check
        .route("/healthz", get(auth::healthz))
        .route("/login", get(auth::login))
        .route("/oauth/callback", get(auth::oauth_callback))
        .route("/logout", post(auth::logout))
        .nest_service("/pkg", ServeDir::new("target/site/pkg"))
        // Include trace context as header in the response
        .layer(OtelInResponseLayer::default())
        // Start OpenTelemetry trace on incoming request (HTTP semantic conventions)
        .layer(OtelAxumLayer::default())
        // Record HTTP metrics (http.server.request.duration histogram)
        .layer(axum_mw::from_fn(http_metrics))
        .layer(Extension(state.clone()))
        .with_state(state)
}
