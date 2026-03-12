mod config;
mod db;
mod k8s_sync;
mod oauth;
mod web;

use std::{collections::HashMap, sync::Arc};

use anyhow::Result;
use axum::{Router, routing::get};
use kube::Client;
use sqlx::PgPool;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<config::AppConfig>,
    pub db: PgPool,
    pub kube: Client,
    pub oidc_client: oauth::OidcClient,
    pub pending_oauth: Arc<RwLock<HashMap<String, oauth::PendingAuth>>>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(config::AppConfig::from_env()?);
    let db = db::connect(&config.database_url).await?;
    db::migrate(&db).await?;

    let kube = Client::try_default().await?;
    let oidc_client = oauth::build_client(&config.oidc).await?;

    let state = Arc::new(AppState {
        config: Arc::clone(&config),
        db,
        kube,
        oidc_client,
        pending_oauth: Arc::new(RwLock::new(HashMap::new())),
    });

    let _workers = k8s_sync::spawn_sync_workers(Arc::clone(&state), Arc::clone(&config));

    let app = Router::new()
        .route("/", get(web::index))
        .route("/healthz", get(web::healthz))
        .route("/login", get(web::login))
        .route("/oauth/callback", get(web::oauth_callback))
        .route("/logout", axum::routing::post(web::logout))
        .route(
            "/ctf/{ctf_name}/challenge/{challenge_name}/",
            get(web::challenge_page),
        )
        .route(
            "/ctf/{ctf_name}/challenge/{challenge_name}/instance-state",
            get(web::instance_state),
        )
        .route(
            "/ctf/{ctf_name}/challenge/{challenge_name}/spawn",
            axum::routing::post(web::spawn_instance),
        )
        .route(
            "/ctf/{ctf_name}/challenge/{challenge_name}/submit",
            axum::routing::post(web::submit_flag),
        )
        .route(
            "/ctf/{ctf_name}/challenge/{challenge_name}/extend",
            axum::routing::post(web::extend_instance),
        )
        .route(
            "/file/{ctf_name}/{challenge_name}/{file_idx}",
            get(web::download_file),
        )
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "ctf app listening");
    axum::serve(listener, app).await?;
    Ok(())
}
