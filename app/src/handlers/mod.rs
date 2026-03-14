mod auth;
mod files;
mod flags;
mod instances;
mod pages;

use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::services::ServeDir;

use crate::state::AppState;

/// Build the application router with all routes wired to their handlers.
pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(pages::index))
        .route("/healthz", get(auth::healthz))
        .route("/login", get(auth::login))
        .route("/oauth/callback", get(auth::oauth_callback))
        .route("/logout", post(auth::logout))
        .route("/challenge/{challenge_name}/", get(pages::challenge_page))
        .route(
            "/challenge/{challenge_name}/instance-state",
            get(instances::instance_state),
        )
        .route(
            "/challenge/{challenge_name}/spawn",
            post(instances::spawn_instance),
        )
        .route(
            "/challenge/{challenge_name}/submit",
            post(flags::submit_flag),
        )
        .route(
            "/challenge/{challenge_name}/extend",
            post(instances::extend_instance),
        )
        .route(
            "/file/{challenge_name}/{file_idx}",
            get(files::download_file),
        )
        .nest_service("/pkg", ServeDir::new("target/site/pkg"))
        .with_state(state)
}
