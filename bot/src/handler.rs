use std::sync::Arc;

use serenity::all::*;
use serenity::async_trait;
use tracing::{error, info};

use crate::config::BotConfig;
use crate::verify::generate_verify_url;

/// Discord gateway event handler.
pub struct Handler {
    pub config: Arc<BotConfig>,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!(user = %ready.user.name, "bot connected");

        let guild_id = GuildId::new(self.config.guild_id);

        let commands = vec![
            CreateCommand::new("setup-verify")
                .description("Post the account-verification panel in this channel")
                .default_member_permissions(Permissions::ADMINISTRATOR),
            CreateCommand::new("setup-help")
                .description("Post the help-desk ticket panel in this channel")
                .default_member_permissions(Permissions::ADMINISTRATOR),
        ];

        match guild_id.set_commands(&ctx.http, commands).await {
            Ok(cmds) => info!(count = cmds.len(), "registered guild commands"),
            Err(e) => error!(error = %e, "failed to register guild commands"),
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(cmd) => self.handle_command(&ctx, cmd).await,
            Interaction::Component(comp) => self.handle_component(&ctx, comp).await,
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Slash commands
// ---------------------------------------------------------------------------

impl Handler {
    async fn handle_command(&self, ctx: &Context, cmd: CommandInteraction) {
        let result = match cmd.data.name.as_str() {
            "setup-verify" => self.cmd_setup_verify(ctx, &cmd).await,
            "setup-help" => self.cmd_setup_help(ctx, &cmd).await,
            _ => Ok(()),
        };

        if let Err(e) = result {
            error!(command = %cmd.data.name, error = %e, "command failed");
        }
    }

    /// Post a persistent verification panel with a **Verify Account** button.
    async fn cmd_setup_verify(
        &self,
        ctx: &Context,
        cmd: &CommandInteraction,
    ) -> serenity::Result<()> {
        let embed = CreateEmbed::new()
            .title("🔐 Account Verification")
            .description(
                "Link your Discord account to the CTF platform.\n\n\
                 Click the button below to get a personal verification link.\n\
                 ⚠️ You must be **logged in** to the CTF website first!",
            )
            .color(0x5865F2);

        let button = CreateButton::new("verify_account")
            .label("Verify Account")
            .style(ButtonStyle::Primary)
            .emoji('🔗');

        let msg = CreateMessage::new()
            .embed(embed)
            .components(vec![CreateActionRow::Buttons(vec![button])]);

        cmd.channel_id.send_message(&ctx.http, msg).await?;

        cmd.create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("✅ Verification panel posted!")
                    .ephemeral(true),
            ),
        )
        .await
    }

    /// Post a persistent help-desk panel with a **Create Ticket** button.
    async fn cmd_setup_help(
        &self,
        ctx: &Context,
        cmd: &CommandInteraction,
    ) -> serenity::Result<()> {
        let embed = CreateEmbed::new()
            .title("🎫 Help Desk")
            .description(
                "Need help? Click the button below to create a support ticket.\n\n\
                 A moderator will assist you shortly.",
            )
            .color(0x57F287);

        let button = CreateButton::new("create_ticket")
            .label("Create Ticket")
            .style(ButtonStyle::Success)
            .emoji('📩');

        let msg = CreateMessage::new()
            .embed(embed)
            .components(vec![CreateActionRow::Buttons(vec![button])]);

        cmd.channel_id.send_message(&ctx.http, msg).await?;

        cmd.create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("✅ Help-desk panel posted!")
                    .ephemeral(true),
            ),
        )
        .await
    }
}

// ---------------------------------------------------------------------------
// Button interactions
// ---------------------------------------------------------------------------

impl Handler {
    async fn handle_component(&self, ctx: &Context, comp: ComponentInteraction) {
        let result = match comp.data.custom_id.as_str() {
            "verify_account" => self.btn_verify(ctx, &comp).await,
            "create_ticket" => self.btn_create_ticket(ctx, &comp).await,
            _ => Ok(()),
        };

        if let Err(e) = result {
            error!(button = %comp.data.custom_id, error = %e, "button interaction failed");
        }
    }

    /// Generate a personal verification URL and send it as an ephemeral message.
    async fn btn_verify(&self, ctx: &Context, comp: &ComponentInteraction) -> serenity::Result<()> {
        let user = &comp.user;
        let url = generate_verify_url(
            &self.config.ctf_app_url,
            &self.config.discord_link_secret,
            &user.id.to_string(),
            &user.name,
        );

        let embed = CreateEmbed::new()
            .title("🔗 Verify Your Account")
            .description(format!(
                "Click the link below to link your Discord to the CTF platform:\n\n\
                 **[Click here to verify]({url})**\n\n\
                 ⚠️ Make sure you're **logged in** to the CTF website first!\n\
                 This link expires in **5 minutes**.",
            ))
            .color(0x5865F2);

        comp.create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .ephemeral(true),
            ),
        )
        .await
    }

    /// Create a private ticket thread and ping the moderator role.
    async fn btn_create_ticket(
        &self,
        ctx: &Context,
        comp: &ComponentInteraction,
    ) -> serenity::Result<()> {
        let user = &comp.user;
        let guild_id = GuildId::new(self.config.guild_id);

        // Find the moderator role by name.
        let mod_role_id = find_role_by_name(ctx, guild_id, &self.config.moderator_role_name).await;

        let thread_name = format!("help-{}", user.name);

        let thread = comp
            .channel_id
            .create_thread(
                &ctx.http,
                CreateThread::new(thread_name)
                    .kind(ChannelType::PrivateThread)
                    .auto_archive_duration(AutoArchiveDuration::OneDay),
            )
            .await;

        match thread {
            Ok(thread) => {
                // Build the opening message that pings both the user and moderators.
                let ping = if let Some(role_id) = mod_role_id {
                    format!(
                        "<@&{}> — <@{}> created a support ticket. Please assist!",
                        role_id, user.id
                    )
                } else {
                    format!("<@{}> created a support ticket.", user.id)
                };

                let _ = thread
                    .id
                    .send_message(&ctx.http, CreateMessage::new().content(ping))
                    .await;

                comp.create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(format!(
                                "✅ Ticket created! Head to <#{}> to chat with a moderator.",
                                thread.id
                            ))
                            .ephemeral(true),
                    ),
                )
                .await
            }
            Err(e) => {
                error!(error = %e, "failed to create ticket thread");
                comp.create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content(
                                "❌ Failed to create ticket. Please try again or contact an admin.",
                            )
                            .ephemeral(true),
                    ),
                )
                .await
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Resolve a role by its name within a guild.
async fn find_role_by_name(ctx: &Context, guild_id: GuildId, name: &str) -> Option<RoleId> {
    let guild = guild_id.to_partial_guild(&ctx.http).await.ok()?;
    guild.roles.values().find(|r| r.name == name).map(|r| r.id)
}

/// Assign the competitor role to a guild member.
///
/// Called from the HTTP callback server after the web app confirms a successful
/// account link.
pub async fn assign_competitor_role(
    http: &serenity::http::Http,
    guild_id: u64,
    discord_user_id: u64,
    role_name: &str,
) -> anyhow::Result<()> {
    let gid = GuildId::new(guild_id);

    let guild = gid
        .to_partial_guild(http)
        .await
        .map_err(|e| anyhow::anyhow!("failed to fetch guild: {e}"))?;

    let role = guild
        .roles
        .values()
        .find(|r| r.name == role_name)
        .ok_or_else(|| anyhow::anyhow!("role '{role_name}' not found in guild"))?;

    let member = gid
        .member(http, UserId::new(discord_user_id))
        .await
        .map_err(|e| anyhow::anyhow!("member {discord_user_id} not found: {e}"))?;

    member
        .add_role(http, role.id)
        .await
        .map_err(|e| anyhow::anyhow!("failed to add role: {e}"))?;

    info!(
        discord_user_id,
        role = role_name,
        "assigned competitor role"
    );
    Ok(())
}
