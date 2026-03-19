use anyhow::{Context, Result};

#[derive(Clone, Debug)]
pub struct BotConfig {
    /// Discord bot token.
    pub discord_token: String,
    /// Shared HMAC secret (same value as the web app's `DISCORD_LINK_SECRET`).
    pub discord_link_secret: String,
    /// Base URL of the CTF web application (e.g. `https://ctf.scc2026.com`).
    pub ctf_app_url: String,
    /// Discord guild (server) ID.
    pub guild_id: u64,
    /// Role name assigned to moderators (default: `scc2026-moderator`).
    pub moderator_role_name: String,
    /// Role name assigned after successful verification (default: `scc2026-competitor`).
    pub competitor_role_name: String,
    /// Bind address for the HTTP callback server (default: `0.0.0.0:3030`).
    pub http_addr: String,
    /// `PostgreSQL` database URL (same DB as the web app).
    pub database_url: String,
    /// How often (in seconds) the reconciler checks for missing roles (default: 60).
    pub reconcile_interval_secs: u64,
}

impl BotConfig {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            discord_token: std::env::var("DISCORD_TOKEN").context("DISCORD_TOKEN is required")?,
            discord_link_secret: std::env::var("DISCORD_LINK_SECRET")
                .context("DISCORD_LINK_SECRET is required")?,
            ctf_app_url: std::env::var("CTF_APP_URL").context("CTF_APP_URL is required")?,
            guild_id: std::env::var("GUILD_ID")
                .context("GUILD_ID is required")?
                .parse()
                .context("GUILD_ID must be a valid u64")?,
            moderator_role_name: std::env::var("MODERATOR_ROLE_NAME")
                .unwrap_or_else(|_| "scc2026-moderator".to_string()),
            competitor_role_name: std::env::var("COMPETITOR_ROLE_NAME")
                .unwrap_or_else(|_| "scc2026-competitor".to_string()),
            http_addr: std::env::var("BOT_HTTP_ADDR")
                .unwrap_or_else(|_| "0.0.0.0:3030".to_string()),
            database_url: std::env::var("DATABASE_URL").context("DATABASE_URL is required")?,
            reconcile_interval_secs: std::env::var("RECONCILE_INTERVAL_SECS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
        })
    }
}
