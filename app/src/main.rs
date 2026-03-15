#[cfg(feature = "ssr")]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    use app::{config, db, handlers, k8s, oauth, scoreboard, state::AppState};
    use std::{collections::HashMap, net::SocketAddr, sync::Arc};
    use tokio::sync::RwLock;

    let _ = rustls::crypto::ring::default_provider().install_default();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(config::AppConfig::from_env()?);
    let db = db::connect(&config.database_url).await?;
    db::migrate(&db).await?;

    let kube = kube::Client::try_default().await?;
    let oidc_client = oauth::build_client(&config.oidc).await?;

    // Create watchers before building state - they provide the Stores
    let watchers = k8s::sync::AppWatchers::new(&kube, &config.ctf_namespace, &config.ctf_name);

    let state = Arc::new(AppState {
        config: Arc::clone(&config),
        db,
        kube,
        oidc_client,
        pending_oauth: Arc::new(RwLock::new(HashMap::new())),
        instances: watchers.instances.clone(),
        flags: watchers.flags.clone(),
        challenges: watchers.challenges.clone(),
        ctfs: watchers.ctfs.clone(),
        scoreboard: Arc::new(RwLock::new(scoreboard::Scoreboard::default())),
    });

    let _workers = k8s::sync::spawn_sync_workers(Arc::clone(&state), Arc::clone(&config), watchers);
    let _scoreboard_worker = scoreboard::spawn_scoreboard_worker(Arc::clone(&state));

    let app = handlers::router(state);

    let listener = tokio::net::TcpListener::bind(&config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "ctf app listening");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

#[cfg(not(feature = "ssr"))]
pub fn main() {}
