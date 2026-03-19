mod config;
mod handler;
mod reconcile;
mod verify;
mod web;

use std::sync::Arc;

use anyhow::Result;
use serenity::all::*;
use tracing::info;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .init();

    let config = Arc::new(config::BotConfig::from_env()?);
    info!("loaded config (guild_id={})", config.guild_id);

    // -- Database pool ---------------------------------------------------

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(3)
        .connect(&config.database_url)
        .await?;
    info!("connected to database");

    // -- Discord client -------------------------------------------------

    let intents = GatewayIntents::GUILDS | GatewayIntents::GUILD_MEMBERS;

    let event_handler = handler::Handler {
        config: Arc::clone(&config),
    };

    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(event_handler)
        .await?;

    // Grab the HTTP handle *before* start() takes ownership.
    let discord_http = Arc::clone(&client.http);

    // -- Reconcile loop -------------------------------------------------

    {
        let config = Arc::clone(&config);
        let http = Arc::clone(&discord_http);
        tokio::spawn(reconcile::run(config, pool, http));
    }

    // -- HTTP callback server -------------------------------------------

    let web_state = Arc::new(web::WebState {
        config: Arc::clone(&config),
        http: discord_http,
    });

    let http_addr = config.http_addr.clone();
    tokio::spawn(async move {
        let app = web::router(web_state);
        let listener = tokio::net::TcpListener::bind(&http_addr)
            .await
            .expect("failed to bind HTTP callback server");
        info!(addr = %http_addr, "HTTP callback server listening");
        axum::serve(listener, app)
            .await
            .expect("HTTP server crashed");
    });

    // -- Start the gateway connection -----------------------------------

    info!("starting Discord gateway…");
    client.start().await?;

    Ok(())
}
