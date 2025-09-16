use serenity::async_trait;
use serenity::model::prelude::*;
use serenity::prelude::*;
use sqlx::SqlitePool;
use tracing::{info, error};
use anyhow::Result;

use crate::commands::SetupCommand;

pub struct Handler {
    db: SqlitePool,
}

impl Handler {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
        
        // Register slash commands globally
        if let Err(e) = self.register_commands(&ctx).await {
            error!("Failed to register commands: {}", e);
        }
    }

    async fn guild_create(&self, _ctx: Context, guild: Guild, _is_new: Option<bool>) {
        info!("Joined guild: {} ({})", guild.name, guild.id);
        
        // Initialize guild in database if not exists
        if let Err(e) = self.ensure_guild_exists(guild.id.get() as i64).await {
            error!("Failed to initialize guild {}: {}", guild.id, e);
        }
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command_interaction) => {
                if let Err(e) = self.handle_command(&ctx, &command_interaction).await {
                    error!("Error handling command: {}", e);
                }
            }
            Interaction::Component(component_interaction) => {
                if let Err(e) = self.handle_component(&ctx, &component_interaction).await {
                    error!("Error handling component: {}", e);
                }
            }
            _ => {}
        }
    }

    async fn message(&self, ctx: Context, msg: Message) {
        // Delete user messages in reservation channels (except bot messages)
        if msg.author.bot {
            return;
        }

        // Check if this is a reservation channel
        if let Some(guild_id) = msg.guild_id {
            let guild_id_i64 = guild_id.get() as i64;
            let channel_id_i64 = msg.channel_id.get() as i64;

            let is_reservation_channel: Option<i64> = sqlx::query_scalar(
                "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
            )
            .bind(guild_id_i64)
            .bind(channel_id_i64)
            .fetch_optional(&self.db)
            .await
            .unwrap_or(None);

            if is_reservation_channel.is_some() {
                if let Err(e) = msg.delete(&ctx.http).await {
                    error!("Failed to delete user message in reservation channel: {}", e);
                }
            }
        }
    }
}

impl Handler {
    async fn register_commands(&self, ctx: &Context) -> Result<()> {
        let commands = vec![
            SetupCommand::register(),
        ];

        serenity::all::Command::set_global_commands(&ctx.http, commands).await?;
        info!("Registered global slash commands");
        Ok(())
    }

    async fn ensure_guild_exists(&self, guild_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR IGNORE INTO guilds (id) VALUES (?)"
        )
        .bind(guild_id)
        .execute(&self.db)
        .await?;
        
        Ok(())
    }

    async fn handle_command(&self, ctx: &Context, interaction: &CommandInteraction) -> Result<()> {
        match interaction.data.name.as_str() {
            "setup" => SetupCommand::handle(ctx, interaction, &self.db).await?,
            _ => {
                error!("Unknown command: {}", interaction.data.name);
            }
        }
        Ok(())
    }

    async fn handle_component(&self, ctx: &Context, interaction: &ComponentInteraction) -> Result<()> {
        match interaction.data.custom_id.as_str() {
            "setup_confirm" => {
                SetupCommand::handle_confirmation(ctx, interaction, &self.db, true).await?
            }
            "setup_cancel" => {
                SetupCommand::handle_confirmation(ctx, interaction, &self.db, false).await?
            }
            "overall_management" => {
                // TODO: Implement overall management UI
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("🚧 Overall Management UI coming soon!")
                        .ephemeral(true)
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            _ => {
                error!("Unknown component interaction: {}", interaction.data.custom_id);
            }
        }
        Ok(())
    }
}