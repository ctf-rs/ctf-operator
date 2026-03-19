//! Periodic reconciliation loop.
//!
//! Checks the database for users that have `discord_id` set but are missing
//! the competitor role in Discord, and assigns it automatically.

use std::sync::Arc;
use std::time::Duration;

use serenity::all::{GuildId, RoleId, UserId};
use sqlx::{PgPool, Row};
use tracing::{error, info, warn};

use crate::config::BotConfig;

/// Start the reconciliation loop.  Runs forever, sleeping
/// `config.reconcile_interval_secs` between each pass.
pub async fn run(config: Arc<BotConfig>, pool: PgPool, http: Arc<serenity::http::Http>) {
    let interval = Duration::from_secs(config.reconcile_interval_secs);
    let guild_id = GuildId::new(config.guild_id);

    info!(
        interval_secs = config.reconcile_interval_secs,
        "reconcile loop started"
    );

    loop {
        tokio::time::sleep(interval).await;

        if let Err(e) = reconcile_once(&config, &pool, &http, guild_id).await {
            error!(error = %e, "reconcile pass failed");
        }
    }
}

async fn reconcile_once(
    config: &BotConfig,
    pool: &PgPool,
    http: &serenity::http::Http,
    guild_id: GuildId,
) -> anyhow::Result<()> {
    // 1. Fetch all users that have a discord_id in the database.
    let rows = sqlx::query("SELECT discord_id FROM users WHERE discord_id IS NOT NULL")
        .fetch_all(pool)
        .await?;

    if rows.is_empty() {
        return Ok(());
    }

    // 2. Resolve the competitor role id in the guild.
    let role_id = resolve_role(http, guild_id, &config.competitor_role_name).await?;

    let mut assigned = 0u32;

    for row in &rows {
        let discord_id: String = row.get("discord_id");
        let Ok(uid) = discord_id.parse::<u64>() else {
            warn!(discord_id = %discord_id, "invalid discord_id in DB, skipping");
            continue;
        };

        // 3. Check if the member already has the role.
        let Ok(member) = guild_id.member(http, UserId::new(uid)).await else {
            // Member may have left the server — skip silently.
            continue;
        };

        if member.roles.contains(&role_id) {
            continue; // already has the role
        }

        // 4. Assign the role.
        if let Err(e) = member.add_role(http, role_id).await {
            warn!(discord_id = %discord_id, error = %e, "failed to assign role");
        } else {
            assigned += 1;
            info!(discord_id = %discord_id, "reconcile: assigned competitor role");
        }
    }

    if assigned > 0 {
        info!(
            assigned,
            total_linked = rows.len(),
            "reconcile pass complete"
        );
    }

    Ok(())
}

/// Look up a role by name and return its ID.
async fn resolve_role(
    http: &serenity::http::Http,
    guild_id: GuildId,
    role_name: &str,
) -> anyhow::Result<RoleId> {
    let guild = guild_id
        .to_partial_guild(http)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch guild: {e}"))?;

    guild
        .roles
        .values()
        .find(|r| r.name == role_name)
        .map(|r| r.id)
        .ok_or_else(|| anyhow::anyhow!("role '{role_name}' not found in guild"))
}
