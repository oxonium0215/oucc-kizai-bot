use anyhow::Result;
use serenity::all::{
    ButtonStyle, ChannelId, CommandInteraction, ComponentInteraction, ComponentInteractionDataKind,
    CreateActionRow, CreateButton, CreateCommand, CreateEmbed, CreateInteractionResponse,
    CreateInteractionResponseMessage, CreateSelectMenu, CreateSelectMenuKind,
    CreateSelectMenuOption, Permissions,
};
use serenity::model::colour::Colour;
use serenity::model::prelude::*;
use serenity::prelude::*;
use sqlx::SqlitePool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

use crate::equipment::EquipmentRenderer;
use crate::utils;

// In-memory storage for setup wizard state
#[derive(Debug, Clone)]
struct SetupWizardState {
    guild_id: GuildId,
    channel_id: ChannelId,
    user_id: UserId,
    selected_roles: Vec<RoleId>,
    // Notification preferences
    dm_fallback_enabled: bool,
    pre_start_minutes: i64,
    pre_end_minutes: i64,
    overdue_repeat_hours: i64,
    overdue_max_count: i64,
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
            states.insert(
                user_id,
                SetupWizardState {
                    guild_id,
                    channel_id,
                    user_id,
                    selected_roles: Vec::new(),
                    // Default notification preferences
                    dm_fallback_enabled: true,
                    pre_start_minutes: 15,
                    pre_end_minutes: 15,
                    overdue_repeat_hours: 12,
                    overdue_max_count: 3,
                },
            );
        }

        // Get guild info to check if there are roles
        let has_roles = if let Some(guild) = guild_id.to_guild_cached(&ctx.cache) {
            let non_everyone_roles_count = guild
                .roles
                .values()
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
            renderer
                .cleanup_duplicate_guides(ctx, guild_id, channel_id.get() as i64)
                .await?;

            // Render equipment display
            renderer
                .reconcile_equipment_display(ctx, guild_id, channel_id.get() as i64)
                .await?;
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
        let selected_role_ids: Vec<RoleId> =
            if let serenity::all::ComponentInteractionDataKind::RoleSelect { values } =
                &interaction.data.kind
            {
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
                .content("✅ Roles selected. Click **Next** to continue."),
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
            let states = SETUP_STATES.lock().await;
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

        // Show notification preferences step
        Self::show_notification_preferences_step(ctx, interaction, db, &state, &selected_roles)
            .await
    }

    async fn show_notification_preferences_step(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
        state: &SetupWizardState,
        selected_roles: &[RoleId],
    ) -> Result<()> {
        // Debug logging to understand state values
        error!("Setup state debug - pre_start: {}, pre_end: {}, overdue: {}, dm_fallback: {}", 
               state.pre_start_minutes, state.pre_end_minutes, state.overdue_repeat_hours, state.dm_fallback_enabled);
        // Update state with selected roles and initialize notification preferences
        {
            let mut states = SETUP_STATES.lock().await;
            if let Some(current_state) = states.get_mut(&state.user_id) {
                current_state.selected_roles = selected_roles.to_vec();
                // Set default notification preferences
                current_state.dm_fallback_enabled = true;
                current_state.pre_start_minutes = 15;
                current_state.pre_end_minutes = 15;
                current_state.overdue_repeat_hours = 12;
                current_state.overdue_max_count = 3;
            }
        }

        let embed = CreateEmbed::new()
            .title("🔧 Setup - Step 3: Notification Preferences")
            .description("Configure reminder notifications for equipment lending:")
            .field(
                "📱 DM Fallback",
                "When DMs fail, send mentions in the reservation channel",
                false,
            )
            .field(
                "⏰ Pre-Start Reminder",
                "Notify users before their reservation starts (default: 15 minutes)",
                false,
            )
            .field(
                "🔔 Pre-End Reminder",
                "Notify users before their reservation ends (default: 15 minutes)",
                false,
            )
            .field(
                "⚠️ Overdue Reminders",
                "Repeat notifications for unreturned items (default: every 12 hours, max 3 times)",
                false,
            )
            .footer(serenity::all::CreateEmbedFooter::new(
                "Current settings shown are defaults. Adjust settings below and click Next to continue.",
            ))
            .color(Colour::BLURPLE);

        // Create separate select menus for each notification type
        let dm_fallback_select = CreateSelectMenu::new(
            "dm_fallback_select",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("Enabled", "dm_fallback_true")
                        .description("Send channel mentions when DMs fail")
                        .default_selection(state.dm_fallback_enabled),
                    CreateSelectMenuOption::new("Disabled", "dm_fallback_false")
                        .description("Only send DMs, no channel fallback")
                        .default_selection(!state.dm_fallback_enabled),
                ],
            },
        )
        .placeholder("DM Fallback Setting")
        .min_values(1)
        .max_values(1);

        let pre_start_select = CreateSelectMenu::new(
            "pre_start_select",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("5 minutes", "pre_start_5")
                        .default_selection(state.pre_start_minutes == 5),
                    CreateSelectMenuOption::new("15 minutes", "pre_start_15")
                        .default_selection(state.pre_start_minutes == 15),
                    CreateSelectMenuOption::new("30 minutes", "pre_start_30")
                        .default_selection(state.pre_start_minutes == 30),
                ],
            },
        )
        .placeholder("Pre-Start Reminder Time")
        .min_values(1)
        .max_values(1);

        let pre_end_select = CreateSelectMenu::new(
            "pre_end_select",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("5 minutes", "pre_end_5")
                        .default_selection(state.pre_end_minutes == 5),
                    CreateSelectMenuOption::new("15 minutes", "pre_end_15")
                        .default_selection(state.pre_end_minutes == 15),
                    CreateSelectMenuOption::new("30 minutes", "pre_end_30")
                        .default_selection(state.pre_end_minutes == 30),
                ],
            },
        )
        .placeholder("Pre-End Reminder Time")
        .min_values(1)
        .max_values(1);

        let overdue_select = CreateSelectMenu::new(
            "overdue_select",
            CreateSelectMenuKind::String {
                options: vec![
                    CreateSelectMenuOption::new("Every 6 hours", "overdue_6h")
                        .default_selection(state.overdue_repeat_hours == 6),
                    CreateSelectMenuOption::new("Every 12 hours", "overdue_12h")
                        .default_selection(state.overdue_repeat_hours == 12),
                    CreateSelectMenuOption::new("Every 24 hours", "overdue_24h")
                        .default_selection(state.overdue_repeat_hours == 24),
                ],
            },
        )
        .placeholder("Overdue Reminder Frequency")
        .min_values(1)
        .max_values(1);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new("setup_notification_back")
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new("notification_next")
                .label("➡️ Next")
                .style(ButtonStyle::Primary),
            CreateButton::new("setup_cancel")
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let components = vec![
            CreateActionRow::SelectMenu(dm_fallback_select),
            CreateActionRow::SelectMenu(pre_start_select),
            CreateActionRow::SelectMenu(pre_end_select),
            CreateActionRow::SelectMenu(overdue_select),
            buttons,
        ];

        let response = CreateInteractionResponse::UpdateMessage(
            CreateInteractionResponseMessage::new()
                .embeds(vec![embed])
                .components(components),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    pub async fn handle_notification_preferences(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
    ) -> Result<()> {
        let user_id = interaction.user.id;
        let values =
            if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
                values.clone()
            } else {
                return Ok(());
            };

        // Update notification preferences in state based on which select menu was used
        {
            let mut states = SETUP_STATES.lock().await;
            if let Some(state) = states.get_mut(&user_id) {
                match interaction.data.custom_id.as_str() {
                    "dm_fallback_select" => {
                        if let Some(value) = values.first() {
                            match value.as_str() {
                                "dm_fallback_true" => state.dm_fallback_enabled = true,
                                "dm_fallback_false" => state.dm_fallback_enabled = false,
                                _ => {}
                            }
                        }
                    }
                    "pre_start_select" => {
                        if let Some(value) = values.first() {
                            match value.as_str() {
                                "pre_start_5" => state.pre_start_minutes = 5,
                                "pre_start_15" => state.pre_start_minutes = 15,
                                "pre_start_30" => state.pre_start_minutes = 30,
                                _ => {}
                            }
                        }
                    }
                    "pre_end_select" => {
                        if let Some(value) = values.first() {
                            match value.as_str() {
                                "pre_end_5" => state.pre_end_minutes = 5,
                                "pre_end_15" => state.pre_end_minutes = 15,
                                "pre_end_30" => state.pre_end_minutes = 30,
                                _ => {}
                            }
                        }
                    }
                    "overdue_select" => {
                        if let Some(value) = values.first() {
                            match value.as_str() {
                                "overdue_6h" => state.overdue_repeat_hours = 6,
                                "overdue_12h" => state.overdue_repeat_hours = 12,
                                "overdue_24h" => state.overdue_repeat_hours = 24,
                                _ => {}
                            }
                        }
                    }
                    _ => {
                        // Handle legacy "notification_preferences" for backwards compatibility
                        for value in &values {
                            match value.as_str() {
                                "dm_fallback_true" => state.dm_fallback_enabled = true,
                                "dm_fallback_false" => state.dm_fallback_enabled = false,
                                "pre_start_5" => state.pre_start_minutes = 5,
                                "pre_start_15" => state.pre_start_minutes = 15,
                                "pre_start_30" => state.pre_start_minutes = 30,
                                "pre_end_5" => state.pre_end_minutes = 5,
                                "pre_end_15" => state.pre_end_minutes = 15,
                                "pre_end_30" => state.pre_end_minutes = 30,
                                "overdue_6h" => state.overdue_repeat_hours = 6,
                                "overdue_12h" => state.overdue_repeat_hours = 12,
                                "overdue_24h" => state.overdue_repeat_hours = 24,
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // Get updated state and refresh the UI to show the new selections
        let (updated_state, selected_roles) = {
            let states = SETUP_STATES.lock().await;
            if let Some(state) = states.get(&user_id) {
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

        // Re-render the notification preferences step with updated defaults
        Self::show_notification_preferences_step(ctx, interaction, _db, &updated_state, &selected_roles).await
    }

    pub async fn handle_notification_next(
        ctx: &Context,
        interaction: &ComponentInteraction,
        db: &SqlitePool,
    ) -> Result<()> {
        let user_id = interaction.user.id;

        // Get current state
        let (state, selected_roles) = {
            let states = SETUP_STATES.lock().await;
            if let Some(state) = states.get(&user_id) {
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
            selected_roles
                .iter()
                .map(|role_id| role_id.mention().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        };

        let notification_summary =
            format!(
            "DM Fallback: {}\nPre-Start: {} min\nPre-End: {} min\nOverdue: Every {} hrs (max {})",
            if state.dm_fallback_enabled { "Enabled" } else { "Disabled" },
            state.pre_start_minutes,
            state.pre_end_minutes,
            state.overdue_repeat_hours,
            state.overdue_max_count
        );

        let embed = CreateEmbed::new()
            .title("🔧 Setup - Step 4: Final Confirmation")
            .description("Please review your configuration:")
            .field(
                "Reservation Channel",
                state.channel_id.mention().to_string(),
                false,
            )
            .field("Admin Roles", role_mentions, false)
            .field("Notification Settings", notification_summary, false)
            .footer(serenity::all::CreateEmbedFooter::new(
                "Click Complete to finish setup or Cancel to abort.",
            ))
            .color(Colour::BLURPLE);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new("setup_final_back")
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
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
            let role_ids: Vec<String> = selected_roles
                .iter()
                .map(|role_id| role_id.get().to_string())
                .collect();
            serde_json::to_string(&role_ids)?
        };

        // Save configuration to database - update existing or insert new
        let result = sqlx::query!(
            "UPDATE guilds SET 
             reservation_channel_id = ?, admin_roles = ?, dm_fallback_channel_enabled = ?,
             pre_start_minutes = ?, pre_end_minutes = ?, overdue_repeat_hours = ?, 
             overdue_max_count = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
            channel_id_i64,
            admin_roles_json,
            state.dm_fallback_enabled,
            state.pre_start_minutes,
            state.pre_end_minutes,
            state.overdue_repeat_hours,
            state.overdue_max_count,
            guild_id_i64
        )
        .execute(db)
        .await?;

        // If no rows updated, insert new record
        if result.rows_affected() == 0 {
            sqlx::query!(
                "INSERT INTO guilds 
                 (id, reservation_channel_id, admin_roles, dm_fallback_channel_enabled,
                  pre_start_minutes, pre_end_minutes, overdue_repeat_hours, overdue_max_count)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                guild_id_i64,
                channel_id_i64,
                admin_roles_json,
                state.dm_fallback_enabled,
                state.pre_start_minutes,
                state.pre_end_minutes,
                state.overdue_repeat_hours,
                state.overdue_max_count
            )
            .execute(db)
            .await?;
        }

        // Show completion message
        let role_summary = if selected_roles.is_empty() {
            "Only Discord administrators will have management access.".to_string()
        } else {
            format!(
                "Admin roles: {}",
                selected_roles
                    .iter()
                    .map(|role_id| role_id.mention().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };

        let notification_summary = format!(
            "DM Fallback: {}, Pre-Start: {}min, Pre-End: {}min, Overdue: Every {}hrs (max {})",
            if state.dm_fallback_enabled {
                "✅"
            } else {
                "❌"
            },
            state.pre_start_minutes,
            state.pre_end_minutes,
            state.overdue_repeat_hours,
            state.overdue_max_count
        );

        let embed = CreateEmbed::new()
            .title("✅ Setup Complete!")
            .description(format!(
                "Successfully configured {} as the reservation channel.\n\n\
                {}\n\
                📱 **Notifications:** {}\n\n\
                🚀 **Next Steps:**\n\
                • Use the **Overall Management** button to add equipment\n\
                • Configure lending/return locations\n\
                • Set up equipment tags for organization",
                state.channel_id.mention(),
                role_summary,
                notification_summary
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
            guild_id_i64,
            channel_id_i64,
            selected_roles.len()
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

    /// Handle back button from notification preferences step to role selection step
    pub async fn handle_notification_back(
        ctx: &Context,
        interaction: &ComponentInteraction,
        _db: &SqlitePool,
    ) -> Result<()> {
        // Go back to role selection step
        Self::show_role_selection_step(ctx, interaction, _db).await
    }

    /// Handle back button from final confirmation step to notification preferences step
    pub async fn handle_final_back(
        ctx: &Context,
        interaction: &ComponentInteraction,
        db: &SqlitePool,
    ) -> Result<()> {
        let user_id = interaction.user.id;

        // Get current state and selected roles
        let (state, selected_roles) = {
            let states = SETUP_STATES.lock().await;
            if let Some(state) = states.get(&user_id) {
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

        // Go back to notification preferences step
        Self::show_notification_preferences_step(ctx, interaction, db, &state, &selected_roles).await
    }
}
