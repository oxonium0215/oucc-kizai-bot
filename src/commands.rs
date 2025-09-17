use anyhow::Result;
use serenity::all::{
    ButtonStyle, ChannelId, CommandInteraction, ComponentInteraction, CreateActionRow,
    CreateButton, CreateCommand, CreateEmbed, CreateInteractionResponse,
    CreateInteractionResponseMessage, Permissions, CreateSelectMenu, CreateSelectMenuKind,
};
use serenity::model::colour::Colour;
use serenity::model::prelude::*;
use serenity::prelude::*;
use sqlx::SqlitePool;
use tracing::{error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::utils;
use crate::equipment::EquipmentRenderer;

// In-memory storage for setup wizard state
#[derive(Debug, Clone)]
struct SetupWizardState {
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
    selected_roles: Vec<RoleId>,
}

lazy_static::lazy_static! {
    static ref SETUP_STATES: Arc<Mutex<HashMap<UserId, SetupWizardState>>> = Arc::new(Mutex::new(HashMap::new()));
}

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
        _db: &SqlitePool,
    ) -> Result<()> {
        // Check if user has admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this command.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Check bot permissions first
        let missing_permissions = utils::check_bot_permissions(ctx, interaction.channel_id).await?;
        if !missing_permissions.is_empty() {
            let response = CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content(format!(
                        "❌ **Permission Error**\n\nThe bot is missing the following required permissions in this channel:\n• {}\n\nPlease grant these permissions and try again.",
                        missing_permissions.join("\n• ")
                    ))
                    .ephemeral(true),
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
                .ephemeral(true),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    pub async fn handle_confirmation(
        ctx: &Context,
        interaction: &ComponentInteraction,
        db: &SqlitePool,
        confirmed: bool,
    ) -> Result<()> {
        if !confirmed {
            let response = CreateInteractionResponse::UpdateMessage(
                CreateInteractionResponseMessage::new()
                    .content("❌ Setup cancelled.")
                    .embeds(vec![])
                    .components(vec![]),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Proceed to role selection step
        Self::show_role_selection_step(ctx, interaction, db).await
    }

    async fn show_role_selection_step(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
    ) -> Result<()> {
        let guild_id = interaction.guild_id.unwrap();
        let channel_id = interaction.channel_id;
        let user_id = interaction.user.id;

        // Store wizard state
        {
            let mut states = SETUP_STATES.lock().await;
            states.insert(user_id, SetupWizardState {
                guild_id,
                channel_id,
                user_id,
                selected_roles: Vec::new(),
            });
        }

        // Get guild info to check if there are roles
        let has_roles = if let Some(guild) = guild_id.to_guild_cached(&ctx.cache) {
            let non_everyone_roles_count = guild.roles.values()
                .filter(|role| !role.name.starts_with('@'))
                .filter(|role| role.id != guild_id.everyone_role())
                .count();
            non_everyone_roles_count > 0
        } else {
            false
        };

        let embed = CreateEmbed::new()
            .title("🔧 Setup - Step 1: Admin Roles")
            .description("Select which roles should have administrative permissions for equipment management.\n\n**Optional**: You can skip this step if you only want Discord administrators to manage equipment.")
            .color(Colour::BLURPLE);

        let mut components = vec![];

        // Create role select menu if there are roles
        if has_roles {
            let role_select = CreateSelectMenu::new(
                "setup_roles_select",
                CreateSelectMenuKind::Role {
                    default_roles: Some(vec![]),
                },
            )
            .placeholder("Select admin roles (optional)")
            .min_values(0)
            .max_values(25);

            components.push(CreateActionRow::SelectMenu(role_select));
        }

        // Buttons row
        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new("setup_roles_skip")
                .label("⏭️ Skip")
                .style(ButtonStyle::Secondary),
            CreateButton::new("setup_roles_next")
                .label("➡️ Next")
                .style(ButtonStyle::Primary),
        ]);
        components.push(buttons);

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(components),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn initialize_channel(
        ctx: &Context,
        channel_id: ChannelId,
        db: &SqlitePool,
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
        let equipment_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM equipment WHERE guild_id = ?")
                .bind(guild_id)
                .fetch_one(db)
                .await?;

        if equipment_count == 0 {
            // Post guide message with Overall Management button
            let embed = CreateEmbed::new()
                .title("📋 Equipment Lending Management")
                .description("Please register equipment to get started.")
                .color(Colour::BLUE);

            let buttons = CreateActionRow::Buttons(vec![CreateButton::new("overall_mgmt_open")
                .label("⚙️ Overall Management")
                .style(ButtonStyle::Primary)]);

            let message = channel_id
                .send_message(
                    &ctx.http,
                    serenity::all::CreateMessage::new()
                        .embed(embed)
                        .components(vec![buttons]),
                )
                .await?;

            // Save message reference
            sqlx::query(
                "INSERT INTO managed_messages (guild_id, channel_id, message_id, message_type, sort_order) VALUES (?, ?, ?, ?, ?)"
            )
            .bind(guild_id)
            .bind(channel_id.get() as i64)
            .bind(message.id.get() as i64)
            .bind("Header")
            .bind(0)
            .execute(db)
            .await?;
        } else {
            // Create equipment embeds using the renderer
            let renderer = EquipmentRenderer::new(db.clone());
            
            // Clean up any duplicate guide messages first
            renderer.cleanup_duplicate_guides(ctx, guild_id, channel_id.get() as i64).await?;
            
            // Render equipment display
            renderer.reconcile_equipment_display(ctx, guild_id, channel_id.get() as i64).await?;
        }

        Ok(())
    }

    pub async fn handle_role_selection(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
    ) -> Result<()> {
        let user_id = interaction.user.id;

        // Update the selected roles in state
        {
            let states = SETUP_STATES.lock().await;
            if let Some(_state) = states.get(&user_id) {
                // Role selection handling is done by the next button handler
                // For now, just acknowledge the selection
            } else {
                // State not found, respond with error
                let response = CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content("❌ Session expired. Please start setup again.")
                        .embeds(vec![])
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        }

        // Acknowledge the selection but don't update the message yet
        // The actual selection values are in interaction.data.values for select menus
        let selected_role_ids: Vec<RoleId> = if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } = &interaction.data.kind {
            values.clone()
        } else {
            Vec::new()
        };

        // Update state with selected roles
        {
            let mut states = SETUP_STATES.lock().await;
            if let Some(state) = states.get_mut(&user_id) {
                state.selected_roles = selected_role_ids;
            }
        }

        // Acknowledge the selection with a simple response
        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .content("✅ Roles selected. Click **Next** to continue.")
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    pub async fn handle_role_skip_or_next(
        ctx: &Context,
        interaction: &ComponentInteraction,
        db: &SqlitePool,
        skip: bool,
    ) -> Result<()> {
        let user_id = interaction.user.id;

        // Get current state
        let (state, selected_roles) = {
            let mut states = SETUP_STATES.lock().await;
            if let Some(state) = states.get(&user_id) {
                let roles = if skip {
                    Vec::new()
                } else {
                    state.selected_roles.clone()
                };
                (state.clone(), roles)
            } else {
                // State not found, respond with error
                let response = CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content("❌ Session expired. Please start setup again.")
                        .embeds(vec![])
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Show final confirmation step
        Self::show_final_confirmation(ctx, interaction, db, &state, &selected_roles).await
    }

    async fn show_final_confirmation(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
        state: &SetupWizardState,
        selected_roles: &[RoleId],
    ) -> Result<()> {
        let role_mentions = if selected_roles.is_empty() {
            "None (only Discord administrators)".to_string()
        } else {
            selected_roles.iter()
                .map(|role_id| role_id.mention().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };

        let embed = CreateEmbed::new()
            .title("🔧 Setup - Step 2: Final Confirmation")
            .description("Please review your configuration:")
            .field("Reservation Channel", state.channel_id.mention().to_string(), false)
            .field("Admin Roles", role_mentions, false)
            .footer(serenity::all::CreateEmbedFooter::new("Click Complete to finish setup or Cancel to abort."))
            .color(Colour::BLURPLE);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new("setup_complete")
                .label("✅ Complete")
                .style(ButtonStyle::Success),
            CreateButton::new("setup_cancel")
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons]),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    pub async fn handle_setup_complete(
        ctx: &Context,
        interaction: &ComponentInteraction,
        db: &SqlitePool,
    ) -> Result<()> {
        let user_id = interaction.user.id;

        // Get and remove state
        let (state, selected_roles) = {
            let mut states = SETUP_STATES.lock().await;
            if let Some(state) = states.remove(&user_id) {
                (state.clone(), state.selected_roles.clone())
            } else {
                // State not found, respond with error
                let response = CreateInteractionResponse::UpdateMessage(
                    CreateInteractionResponseMessage::new()
                        .content("❌ Session expired. Please start setup again.")
                        .embeds(vec![])
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        let guild_id_i64 = state.guild_id.get() as i64;
        let channel_id_i64 = state.channel_id.get() as i64;

        // Convert selected roles to JSON
        let admin_roles_json = if selected_roles.is_empty() {
            "[]".to_string()
        } else {
            let role_ids: Vec<String> = selected_roles.iter()
                .map(|role_id| role_id.get().to_string())
                .collect();
            serde_json::to_string(&role_ids)?
        };

        // Save configuration to database
        sqlx::query(
            "INSERT OR REPLACE INTO guilds (id, reservation_channel_id, admin_roles) VALUES (?, ?, ?)"
        )
        .bind(guild_id_i64)
        .bind(channel_id_i64)
        .bind(&admin_roles_json)
        .execute(db)
        .await?;

        // Show completion message
        let role_summary = if selected_roles.is_empty() {
            "Only Discord administrators will have management access.".to_string()
        } else {
            format!("Admin roles: {}", 
                selected_roles.iter()
                    .map(|role_id| role_id.mention().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        let embed = CreateEmbed::new()
            .title("✅ Setup Complete!")
            .description(format!(
                "Successfully configured {} as the reservation channel.\n\n\
                {}\n\n\
                🚀 **Next Steps:**\n\
                • Use the **Overall Management** button to add equipment\n\
                • Configure lending/return locations\n\
                • Set up equipment tags for organization",
                state.channel_id.mention(),
                role_summary
            ))
            .color(Colour::DARK_GREEN);

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![]),
        );

        interaction.create_response(&ctx.http, response).await?;

        // Initialize channel with guide message or equipment embeds
        Self::initialize_channel(ctx, state.channel_id, db).await?;

        info!(
            "Setup completed for guild {} in channel {} with {} admin roles",
            guild_id_i64, channel_id_i64, selected_roles.len()
        );
        Ok(())
    }

    pub async fn handle_setup_cancel(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
    ) -> Result<()> {
        let user_id = interaction.user.id;

        // Remove state
        {
            let mut states = SETUP_STATES.lock().await;
            states.remove(&user_id);
        }

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .content("❌ Setup cancelled.")
                .embeds(vec![])
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }
}
