use anyhow::Result;
use serenity::all::{
    CreateCommand, CreateInteractionResponse, CreateInteractionResponseMessage, 
    CreateEmbed, CreateActionRow, CreateButton, ButtonStyle,
    CommandInteraction, ComponentInteraction, Permissions, ChannelId
};
use serenity::model::colour::Colour;
use serenity::model::prelude::*;
use serenity::prelude::*;
use sqlx::SqlitePool;
use tracing::{info, error, warn};

use crate::utils;

pub struct SetupCommand;

impl SetupCommand {
    pub fn register() -> CreateCommand {
        CreateCommand::new("setup")
            .description("Set up the equipment lending bot in this channel")
            .default_member_permissions(Permissions::ADMINISTRATOR)
    }

    pub async fn handle(
        ctx: &Context, 
        interaction: &CommandInteraction, 
        _db: &SqlitePool
    ) -> Result<()> {
        // Check if user has admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this command.")
                    .ephemeral(true)
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Create confirmation embed
        let embed = CreateEmbed::new()
            .title("🔧 Equipment Lending Bot Setup")
            .description(format!(
                "Set {} as the reservation channel. Is that okay?\n\n\
                ⚠️ **Warning**: This will delete all existing messages in this channel except bot-managed messages.",
                interaction.channel_id.mention()
            ))
            .color(Colour::BLURPLE);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new("setup_confirm")
                .label("✅ Confirm")
                .style(ButtonStyle::Success),
            CreateButton::new("setup_cancel")
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let response = CreateInteractionResponse::Message(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons])
                .ephemeral(true)
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    pub async fn handle_confirmation(
        ctx: &Context,
        interaction: &ComponentInteraction,
        db: &SqlitePool,
        confirmed: bool
    ) -> Result<()> {
        if !confirmed {
            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("❌ Setup cancelled.")
                    .embeds(vec![])
                    .components(vec![])
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Check bot permissions - simplified for now
        // TODO: Implement proper permission checking

        // Complete setup directly for now (skip role selection)
        let guild_id = interaction.guild_id.unwrap();
        let channel_id = interaction.channel_id;
        let guild_id_i64 = guild_id.get() as i64;
        let channel_id_i64 = channel_id.get() as i64;

        // Save configuration to database
        sqlx::query(
            "INSERT OR REPLACE INTO guilds (id, reservation_channel_id, admin_roles) VALUES (?, ?, ?)"
        )
        .bind(guild_id_i64)
        .bind(channel_id_i64)
        .bind("[]") // Empty admin roles for now
        .execute(db)
        .await?;

        // Show completion message
        let embed = CreateEmbed::new()
            .title("✅ Setup Complete!")
            .description(format!(
                "Successfully configured {} as the reservation channel.\n\n\
                🚀 **Next Steps:**\n\
                • Use the **Overall Management** button to add equipment\n\
                • Configure lending/return locations\n\
                • Set up equipment tags for organization",
                channel_id.mention()
            ))
            .color(Colour::DARK_GREEN);

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![])
        );

        interaction.create_response(&ctx.http, response).await?;

        // Initialize channel with guide message or equipment embeds
        Self::initialize_channel(ctx, channel_id, db).await?;

        info!("Setup completed for guild {} in channel {}", guild_id_i64, channel_id_i64);
        Ok(())
    }

    async fn initialize_channel(
        ctx: &Context,
        channel_id: ChannelId,
        db: &SqlitePool
    ) -> Result<()> {
        // Get guild ID from channel
        let channel = channel_id.to_channel(&ctx.http).await?;
        let guild_id = if let Some(guild_channel) = channel.guild() {
            guild_channel.guild_id.get() as i64
        } else {
            error!("Failed to get guild ID for channel {}", channel_id);
            return Ok(());
        };

        // Check if there's any equipment in this guild
        let equipment_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM equipment WHERE guild_id = ?"
        )
        .bind(guild_id)
        .fetch_one(db)
        .await?;

        if equipment_count == 0 {
            // Post guide message with Overall Management button
            let embed = CreateEmbed::new()
                .title("📋 Equipment Lending Management")
                .description("Please register equipment to get started.")
                .color(Colour::BLUE);

            let buttons = CreateActionRow::Buttons(vec![
                CreateButton::new("overall_management")
                    .label("⚙️ Overall Management")
                    .style(ButtonStyle::Primary),
            ]);

            let message = channel_id.send_message(&ctx.http, 
                serenity::all::CreateMessage::new()
                    .embed(embed)
                    .components(vec![buttons])
            ).await?;

            // Save message reference
            sqlx::query(
                "INSERT INTO managed_messages (guild_id, channel_id, message_id, message_type) VALUES (?, ?, ?, ?)"
            )
            .bind(guild_id)
            .bind(channel_id.get() as i64)
            .bind(message.id.get() as i64)
            .bind("Guide")
            .execute(db)
            .await?;
        } else {
            // TODO: Create equipment embeds
            warn!("Equipment embed creation not yet implemented");
        }

        Ok(())
    }
}