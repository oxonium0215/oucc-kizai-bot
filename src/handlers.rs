use anyhow::Result;
use serenity::async_trait;
use serenity::model::prelude::*;
use serenity::prelude::*;
use serenity::all::ComponentInteractionDataKind;
use sqlx::SqlitePool;
use tracing::{error, info};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use chrono::{DateTime, Utc};

use crate::commands::SetupCommand;
use crate::utils;
use crate::equipment::EquipmentRenderer;

// In-memory storage for reservation wizard state
#[derive(Debug, Clone)]
struct ReservationWizardState {
    equipment_id: i64,
    user_id: UserId,
    guild_id: GuildId,
    step: WizardStep,
    start_time: Option<DateTime<Utc>>,
    end_time: Option<DateTime<Utc>>,
    location: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum WizardStep {
    StartTime,
    EndTime,
    Location,
    Confirmation,
}

lazy_static::lazy_static! {
    static ref RESERVATION_WIZARD_STATES: Arc<Mutex<HashMap<(UserId, String), ReservationWizardState>>> = Arc::new(Mutex::new(HashMap::new()));
}

// Helper struct for simulating component interactions from modals
#[derive(Clone)]
struct ComponentInteractionRef {
    user: User,
    token: String,
    guild_id: Option<GuildId>,
    channel_id: ChannelId,
}

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

        // Self-healing: reconcile all configured reservation channels
        info!("Starting self-healing reconciliation for all guilds");
        if let Err(e) = self.reconcile_all_guilds(&ctx).await {
            error!("Failed to reconcile all guilds on startup: {}", e);
        }
        info!("Self-healing reconciliation completed");
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
            Interaction::Modal(modal_interaction) => {
                if let Err(e) = self.handle_modal(&ctx, &modal_interaction).await {
                    error!("Error handling modal: {}", e);
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
                    error!(
                        "Failed to delete user message in reservation channel: {}",
                        e
                    );
                }
            }
        }
    }
}

impl Handler {
    /// Self-healing: reconcile equipment displays for all configured guilds
    async fn reconcile_all_guilds(&self, ctx: &Context) -> Result<()> {
        let guilds = sqlx::query!(
            "SELECT id, reservation_channel_id FROM guilds WHERE reservation_channel_id IS NOT NULL"
        )
        .fetch_all(&self.db)
        .await?;

        for guild in guilds {
            let guild_id = guild.id;
            let channel_id = guild.reservation_channel_id.unwrap();
            
            info!("Reconciling equipment display for guild {} in channel {}", guild_id, channel_id);
            
            let renderer = EquipmentRenderer::new(self.db.clone());
            if let Err(e) = renderer.reconcile_equipment_display(ctx, guild_id, channel_id).await {
                error!(
                    "Failed to reconcile equipment display for guild {} channel {}: {}",
                    guild_id, channel_id, e
                );
            }
        }
        
        Ok(())
    }

    /// Get the reservation channel ID for a guild
    async fn get_reservation_channel_id(&self, guild_id: i64) -> Result<i64> {
        let channel_id: Option<i64> = sqlx::query_scalar(
            "SELECT reservation_channel_id FROM guilds WHERE id = ?"
        )
        .bind(guild_id)
        .fetch_optional(&self.db)
        .await?
        .flatten();

        match channel_id {
            Some(id) => Ok(id),
            None => Err(anyhow::anyhow!("No reservation channel configured for guild {}", guild_id))
        }
    }

    async fn register_commands(&self, ctx: &Context) -> Result<()> {
        let commands = vec![SetupCommand::register()];

        serenity::all::Command::set_global_commands(&ctx.http, commands).await?;
        info!("Registered global slash commands");
        Ok(())
    }

    async fn ensure_guild_exists(&self, guild_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT OR IGNORE INTO guilds (id) VALUES (?)")
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

    async fn handle_component(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        match interaction.data.custom_id.as_str() {
            "setup_confirm" => {
                SetupCommand::handle_confirmation(ctx, interaction, &self.db, true).await?
            }
            "setup_cancel" => {
                SetupCommand::handle_setup_cancel(ctx, interaction, &self.db).await?
            }
            "setup_roles_select" => {
                SetupCommand::handle_role_selection(ctx, interaction, &self.db).await?
            }
            "setup_roles_skip" => {
                SetupCommand::handle_role_skip_or_next(ctx, interaction, &self.db, true).await?
            }
            "setup_roles_next" => {
                SetupCommand::handle_role_skip_or_next(ctx, interaction, &self.db, false).await?
            }
            "setup_complete" => {
                SetupCommand::handle_setup_complete(ctx, interaction, &self.db).await?
            }
            "overall_management" | "overall_mgmt_open" => {
                self.handle_overall_management(ctx, interaction).await?
            }
            "mgmt_add_tag" => {
                self.handle_add_tag(ctx, interaction).await?
            }
            "mgmt_add_location" => {
                self.handle_add_location(ctx, interaction).await?
            }
            "mgmt_add_equipment" => {
                self.handle_add_equipment(ctx, interaction).await?
            }
            "mgmt_refresh_display" => {
                self.handle_refresh_display(ctx, interaction).await?
            }
            _ => {
                // Check for dynamic reservation and equipment IDs (support both old and new format)
                if interaction.data.custom_id.starts_with("eq_reserve:") || interaction.data.custom_id.starts_with("reserve_") {
                    self.handle_equipment_reserve(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("change_") {
                    self.handle_equipment_change(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("return_") {
                    self.handle_equipment_return(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("res_edit:") {
                    self.handle_reservation_edit(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("res_cancel:") {
                    self.handle_reservation_cancel(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("res_admin_cancel:") {
                    self.handle_reservation_admin_cancel(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_start_input:") {
                    self.handle_reservation_wizard_start_input(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_end_input:") {
                    self.handle_reservation_wizard_end_input(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_location_input:") {
                    self.handle_reservation_wizard_location_input(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_location_default:") {
                    self.handle_reservation_wizard_location_default(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_location_skip:") {
                    self.handle_reservation_wizard_location_skip(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_back_start:") {
                    self.handle_reservation_wizard_back_start(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_back_end:") {
                    self.handle_reservation_wizard_back_end(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_back_location:") {
                    self.handle_reservation_wizard_back_location(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_confirm:") {
                    self.handle_reservation_wizard_confirm(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_cancel:") {
                    self.handle_reservation_wizard_cancel(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("change_reservation_select:") {
                    self.handle_change_reservation_select(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("change_res_time:") {
                    self.handle_change_reservation_time(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("change_res_location:") {
                    self.handle_change_reservation_location(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("cancel_res:") {
                    self.handle_cancel_reservation_confirm(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("confirm_cancel_res:") {
                    self.handle_confirm_cancel_reservation(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("abort_cancel_res:") {
                    self.handle_abort_cancel_reservation(ctx, interaction).await?
                } else {
                    error!(
                        "Unknown component interaction: {}",
                        interaction.data.custom_id
                    );
                }
            }
        }
        Ok(())
    }

    async fn handle_overall_management(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Check admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this feature.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Create management panel with buttons
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let embed = CreateEmbed::new()
            .title("⚙️ Overall Management")
            .description("Equipment and organization management panel")
            .color(Colour::BLUE);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new("mgmt_add_tag")
                .label("🏷️ Add Tag")
                .style(ButtonStyle::Secondary),
            CreateButton::new("mgmt_add_location")
                .label("📍 Add Location")
                .style(ButtonStyle::Secondary),
            CreateButton::new("mgmt_add_equipment")
                .label("📦 Add Equipment")
                .style(ButtonStyle::Secondary),
            CreateButton::new("mgmt_refresh_display")
                .label("🔄 Refresh Display")
                .style(ButtonStyle::Primary),
        ]);

        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons])
                .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_add_tag(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Check admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this feature.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Create modal for adding tag
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new("add_tag_modal", "Add New Tag")
            .components(vec![
                serenity::all::CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "name", "Tag Name")
                        .placeholder("e.g., Cameras, Audio, Lighting")
                        .required(true)
                        .max_length(50)
                ),
                serenity::all::CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "sort_order", "Sort Order")
                        .placeholder("Number for ordering (e.g., 1, 2, 3...)")
                        .required(true)
                        .max_length(10)
                ),
            ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_add_location(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Check admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this feature.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Create modal for adding location
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new("add_location_modal", "Add New Location")
            .components(vec![
                serenity::all::CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "name", "Location Name")
                        .placeholder("e.g., Office A, Lab B, Storage Room")
                        .required(true)
                        .max_length(100)
                ),
            ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_add_equipment(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Check admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this feature.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // TODO: For now, show a simple modal. In a full implementation, we'd use select menus for tags and locations
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new("add_equipment_modal", "Add New Equipment")
            .components(vec![
                serenity::all::CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "name", "Equipment Name")
                        .placeholder("e.g., Sony A7III, Shure SM58")
                        .required(true)
                        .max_length(100)
                ),
                serenity::all::CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "tag_name", "Tag Name")
                        .placeholder("Enter existing tag name (optional)")
                        .required(false)
                        .max_length(50)
                ),
                serenity::all::CreateActionRow::InputText(
                    CreateInputText::new(InputTextStyle::Short, "location", "Default Return Location")
                        .placeholder("Enter location name (optional)")
                        .required(false)
                        .max_length(100)
                ),
            ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_refresh_display(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Check admin permissions
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this feature.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let guild_id = interaction.guild_id.unwrap().get() as i64;
        let channel_id = interaction.channel_id.get() as i64;

        // Use equipment renderer to refresh the display
        let renderer = EquipmentRenderer::new(self.db.clone());
        match renderer.reconcile_equipment_display(ctx, guild_id, channel_id).await {
            Ok(()) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("✅ Equipment display refreshed successfully!")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
                error!("Failed to refresh equipment display: {}", e);
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Failed to refresh equipment display. Check logs for details.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        match interaction.data.custom_id.as_str() {
            "add_tag_modal" => {
                self.handle_add_tag_modal(ctx, interaction).await?
            }
            "add_location_modal" => {
                self.handle_add_location_modal(ctx, interaction).await?
            }
            "add_equipment_modal" => {
                self.handle_add_equipment_modal(ctx, interaction).await?
            }
            _ => {
                // Check for dynamic reservation modals
                if interaction.data.custom_id.starts_with("reserve_modal:") {
                    self.handle_reservation_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("edit_reservation_modal:") {
                    self.handle_edit_reservation_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_start_time_modal:") {
                    self.handle_reservation_wizard_start_time_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_end_time_modal:") {
                    self.handle_reservation_wizard_end_time_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("reserve_location_modal:") {
                    self.handle_reservation_wizard_location_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("change_time_modal:") {
                    self.handle_change_time_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("change_location_modal:") {
                    self.handle_change_location_modal(ctx, interaction).await?
                } else {
                    error!("Unknown modal interaction: {}", interaction.data.custom_id);
                }
            }
        }
        Ok(())
    }

    async fn handle_add_tag_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let guild_id = interaction.guild_id.unwrap().get() as i64;
        
        // Extract data from modal - access components correctly for Serenity modal structure
        let mut name = String::new();
        let mut sort_order_str = String::new();
        
        for row in &interaction.data.components {
            for component in &row.components {
                // ActionRowComponent is an enum, match on it properly
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    match input_text.custom_id.as_str() {
                        "name" => name = input_text.value.clone().unwrap_or_default(),
                        "sort_order" => sort_order_str = input_text.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        // Validate inputs
        if name.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Tag name is required.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let sort_order: i64 = match sort_order_str.parse() {
            Ok(num) => num,
            Err(_) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Sort order must be a number.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Insert tag into database
        match sqlx::query(
            "INSERT INTO tags (guild_id, name, sort_order) VALUES (?, ?, ?)"
        )
        .bind(guild_id)
        .bind(&name)
        .bind(sort_order)
        .execute(&self.db)
        .await
        {
            Ok(_) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("✅ Tag '{}' added successfully! Use 'Refresh Display' to update the equipment list.", name))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
                error!("Failed to insert tag: {}", e);
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Failed to add tag. It might already exist.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_add_location_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let guild_id = interaction.guild_id.unwrap().get() as i64;
        
        // Extract data from modal
        let mut name = String::new();
        
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    if input_text.custom_id == "name" {
                        name = input_text.value.clone().unwrap_or_default();
                    }
                }
            }
        }

        // Validate inputs
        if name.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Location name is required.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Insert location into database
        match sqlx::query(
            "INSERT INTO locations (guild_id, name) VALUES (?, ?)"
        )
        .bind(guild_id)
        .bind(&name)
        .execute(&self.db)
        .await
        {
            Ok(_) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("✅ Location '{}' added successfully!", name))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
                error!("Failed to insert location: {}", e);
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Failed to add location. It might already exist.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_add_equipment_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let guild_id = interaction.guild_id.unwrap().get() as i64;
        
        // Extract data from modal
        let mut name = String::new();
        let mut tag_name = Option::<String>::None;
        let mut location = Option::<String>::None;
        
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    match input_text.custom_id.as_str() {
                        "name" => name = input_text.value.clone().unwrap_or_default(),
                        "tag_name" => {
                            if let Some(value) = &input_text.value {
                                if !value.is_empty() {
                                    tag_name = Some(value.clone());
                                }
                            }
                        },
                        "location" => {
                            if let Some(value) = &input_text.value {
                                if !value.is_empty() {
                                    location = Some(value.clone());
                                }
                            }
                        },
                        _ => {}
                    }
                }
            }
        }

        // Validate inputs
        if name.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Equipment name is required.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Look up tag ID if tag name provided
        let tag_id: Option<i64> = if let Some(ref tag_name_val) = tag_name {
            sqlx::query_scalar(
                "SELECT id FROM tags WHERE guild_id = ? AND name = ?"
            )
            .bind(guild_id)
            .bind(tag_name_val)
            .fetch_optional(&self.db)
            .await?
        } else {
            None
        };

        // Insert equipment into database
        match sqlx::query(
            "INSERT INTO equipment (guild_id, tag_id, name, status, default_return_location) VALUES (?, ?, ?, ?, ?)"
        )
        .bind(guild_id)
        .bind(tag_id)
        .bind(&name)
        .bind("Available")
        .bind(&location)
        .execute(&self.db)
        .await
        {
            Ok(_) => {
                let mut response_text = format!("✅ Equipment '{}' added successfully!", name);
                if tag_name.is_some() && tag_id.is_none() {
                    response_text.push_str(" (Note: Tag not found, equipment added without tag)");
                }
                response_text.push_str(" Use 'Refresh Display' to update the equipment list.");

                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(response_text)
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(e) => {
                error!("Failed to insert equipment: {}", e);
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Failed to add equipment. It might already exist.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    // Reservation handling methods

    async fn handle_equipment_reserve(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Support both old and new button formats
        let equipment_id_str = interaction.data.custom_id
            .strip_prefix("eq_reserve:")
            .or_else(|| interaction.data.custom_id.strip_prefix("reserve_"))
            .unwrap_or("");
            
        let equipment_id: i64 = equipment_id_str.parse().unwrap_or(0);
        if equipment_id == 0 {
            error!("Invalid equipment ID in reserve button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Check if equipment exists and is available
        let equipment = sqlx::query!(
            "SELECT id, name, status, default_return_location FROM equipment WHERE id = ?",
            equipment_id
        )
        .fetch_optional(&self.db)
        .await?;

        let equipment = match equipment {
            Some(eq) => eq,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Equipment not found.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        if equipment.status != "Available" {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ This equipment is not available for reservation.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Initialize reservation wizard state
        let wizard_state = ReservationWizardState {
            equipment_id,
            user_id: interaction.user.id,
            guild_id: interaction.guild_id.unwrap(),
            step: WizardStep::StartTime,
            start_time: None,
            end_time: None,
            location: None,
        };

        // Store wizard state using user_id and interaction token as key
        let state_key = (interaction.user.id, interaction.token.clone());
        {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            states.insert(state_key, wizard_state);
        }

        // Start wizard with start time step
        self.show_start_time_step(ctx, interaction, &equipment.name).await?;
        Ok(())
    }

    async fn handle_equipment_change(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let equipment_id_str = interaction.data.custom_id
            .strip_prefix("change_")
            .unwrap_or("");
            
        let equipment_id: i64 = equipment_id_str.parse().unwrap_or(0);
        if equipment_id == 0 {
            error!("Invalid equipment ID in change button: {}", interaction.data.custom_id);
            return Ok(());
        }

        let user_id = interaction.user.id.get() as i64;
        
        // Get user's active reservations for this equipment
        let reservations = sqlx::query!(
            "SELECT r.id, r.start_time, r.end_time, r.location, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.equipment_id = ? AND r.user_id = ? AND r.status = 'Confirmed'
             ORDER BY r.start_time ASC",
            equipment_id,
            user_id
        )
        .fetch_all(&self.db)
        .await?;

        if reservations.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You don't have any active reservations for this equipment.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Show reservation selection menu
        use serenity::all::{CreateEmbed, CreateActionRow, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, Colour};
        
        let equipment_name = &reservations[0].equipment_name;
        
        let embed = CreateEmbed::new()
            .title("🔄 Manage Reservations")
            .description(format!("**Equipment:** {}\n\nSelect a reservation to change or cancel:", equipment_name))
            .color(Colour::BLUE);

        let mut options = Vec::new();
        for reservation in &reservations {
            let reservation_id = reservation.id.unwrap_or(0); // ID should always be present for confirmed reservations
            let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
            let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));
            let location_text = reservation.location.as_deref().unwrap_or("No location");
            
            options.push(
                CreateSelectMenuOption::new(
                    format!("{} to {} - {}", start_jst, end_jst, location_text),
                    format!("reservation_{}", reservation_id)
                )
                .description(format!("ID: {}", reservation_id))
            );
        }

        let select_menu = CreateSelectMenu::new(
            format!("change_reservation_select:{}", interaction.token),
            CreateSelectMenuKind::String { options }
        )
        .placeholder("Select a reservation to manage...")
        .max_values(1);

        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![CreateActionRow::SelectMenu(select_menu)])
                .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_equipment_return(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let equipment_id_str = interaction.data.custom_id
            .strip_prefix("return_")
            .unwrap_or("");
            
        let equipment_id: i64 = equipment_id_str.parse().unwrap_or(0);
        if equipment_id == 0 {
            error!("Invalid equipment ID in return button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // For now, just respond with a placeholder message
        // TODO: Implement equipment return functionality in future PR
        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("↩️ Equipment return functionality coming soon!")
                .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    // Reservation wizard step methods

    async fn show_start_time_step(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        equipment_name: &str,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let embed = CreateEmbed::new()
            .title("📅 Reserve Equipment - Step 1/3")
            .description(format!("**Equipment:** {}\n\n**Step 1:** Please enter the start date and time for your reservation.\n\n⏰ **Format:** YYYY-MM-DD HH:MM (JST)\n📝 **Example:** 2024-01-15 14:30\n\n⚠️ **Note:** Start time must be in the future.", equipment_name))
            .color(Colour::BLUE)
            .footer(serenity::all::CreateEmbedFooter::new("Times are in Japan Standard Time (JST)"));

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("reserve_start_input:{}", interaction.token))
                .label("📅 Enter Start Time")
                .style(ButtonStyle::Primary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons])
                .ephemeral(true),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn show_end_time_step(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        equipment_name: &str,
        start_time: DateTime<Utc>,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let start_jst = crate::time::utc_to_jst_string(start_time);
        
        let embed = CreateEmbed::new()
            .title("📅 Reserve Equipment - Step 2/3")
            .description(format!("**Equipment:** {}\n**Start Time:** {}\n\n**Step 2:** Please enter the end date and time for your reservation.\n\n⏰ **Format:** YYYY-MM-DD HH:MM (JST)\n📝 **Example:** 2024-01-15 18:30\n\n⚠️ **Note:** End time must be after start time and within 60 days.", equipment_name, start_jst))
            .color(Colour::BLUE)
            .footer(serenity::all::CreateEmbedFooter::new("Times are in Japan Standard Time (JST)"));

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("reserve_end_input:{}", interaction.token))
                .label("📅 Enter End Time")
                .style(ButtonStyle::Primary),
            CreateButton::new(format!("reserve_back_start:{}", interaction.token))
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons]),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn show_location_step(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        equipment_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        default_location: Option<String>,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let start_jst = crate::time::utc_to_jst_string(start_time);
        let end_jst = crate::time::utc_to_jst_string(end_time);
        
        let embed = CreateEmbed::new()
            .title("📍 Reserve Equipment - Step 3/3")
            .description(format!("**Equipment:** {}\n**Start Time:** {}\n**End Time:** {}\n\n**Step 3:** Please specify the return location (optional).\n\n📍 You can use the default location or enter a custom one.", equipment_name, start_jst, end_jst))
            .color(Colour::BLUE);

        let mut buttons = vec![
            CreateButton::new(format!("reserve_location_input:{}", interaction.token))
                .label("📍 Enter Location")
                .style(ButtonStyle::Primary),
        ];

        if let Some(ref default_loc) = default_location {
            if !default_loc.is_empty() {
                buttons.push(
                    CreateButton::new(format!("reserve_location_default:{}", interaction.token))
                        .label(format!("📍 Use Default ({})", default_loc))
                        .style(ButtonStyle::Secondary)
                );
            }
        }

        buttons.extend_from_slice(&[
            CreateButton::new(format!("reserve_location_skip:{}", interaction.token))
                .label("⏭️ Skip Location")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_back_end:{}", interaction.token))
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![CreateActionRow::Buttons(buttons)]),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn show_confirmation_step(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        equipment_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        location: Option<String>,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let start_jst = crate::time::utc_to_jst_string(start_time);
        let end_jst = crate::time::utc_to_jst_string(end_time);
        let location_text = location.as_deref().unwrap_or("Not specified");
        
        // Check for conflicts in real-time before showing confirmation
        let state_key = (interaction.user.id, interaction.token.clone());
        let equipment_id = {
            let states = RESERVATION_WIZARD_STATES.lock().await;
            states.get(&state_key).map(|s| s.equipment_id).unwrap_or(0)
        };

        if equipment_id == 0 {
            let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Session expired. Please start the reservation process again.")
                    .components(vec![]),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Check for conflicts
        let conflicts = sqlx::query!(
            "SELECT id, user_id, start_time, end_time FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' 
             AND start_time < ? AND end_time > ?",
            equipment_id,
            end_time,
            start_time
        )
        .fetch_all(&self.db)
        .await?;

        if !conflicts.is_empty() {
            let conflict = &conflicts[0];
            let conflict_start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.start_time));
            let conflict_end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.end_time));
            
            let embed = CreateEmbed::new()
                .title("⚠️ Reservation Conflict Detected")
                .description(format!("**Equipment:** {}\n\n❌ **Conflict:** Your requested time overlaps with an existing reservation by <@{}> from {} to {}.\n\nPlease go back and choose different times.", equipment_name, conflict.user_id, conflict_start_jst, conflict_end_jst))
                .color(Colour::RED);

            let buttons = CreateActionRow::Buttons(vec![
                CreateButton::new(format!("reserve_back_location:{}", interaction.token))
                    .label("⬅️ Back to Times")
                    .style(ButtonStyle::Secondary),
                CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                    .label("❌ Cancel")
                    .style(ButtonStyle::Danger),
            ]);

            let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(vec![buttons]),
            );

            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }
        
        let embed = CreateEmbed::new()
            .title("✅ Confirm Reservation")
            .description(format!("**Equipment:** {}\n**Start Time:** {}\n**End Time:** {}\n**Return Location:** {}\n\n🔍 **Conflict Check:** ✅ No conflicts detected\n\nPlease confirm your reservation details.", equipment_name, start_jst, end_jst, location_text))
            .color(Colour::DARK_GREEN);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("reserve_confirm:{}", interaction.token))
                .label("✅ Confirm Reservation")
                .style(ButtonStyle::Success),
            CreateButton::new(format!("reserve_back_location:{}", interaction.token))
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons]),
        );

        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_reservation_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let equipment_id_str = interaction.data.custom_id
            .strip_prefix("reserve_modal:")
            .unwrap_or("");
            
        let equipment_id: i64 = equipment_id_str.parse().unwrap_or(0);
        if equipment_id == 0 {
            error!("Invalid equipment ID in reservation modal: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Extract modal data
        let mut start_time_str = String::new();
        let mut end_time_str = String::new();
        let mut location = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    match input_text.custom_id.as_str() {
                        "start_time" => start_time_str = input_text.value.clone().unwrap_or_default(),
                        "end_time" => end_time_str = input_text.value.clone().unwrap_or_default(),
                        "location" => location = input_text.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        // Parse and validate times
        let (start_utc, end_utc) = match self.parse_and_validate_times(&start_time_str, &end_time_str) {
            Ok(times) => times,
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Create reservation with conflict detection
        match self.create_reservation_with_conflict_check(
            equipment_id,
            interaction.user.id.get() as i64,
            start_utc,
            end_utc,
            if location.is_empty() { None } else { Some(location) },
        ).await {
            Ok(reservation_id) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("✅ Reservation created successfully! (ID: {})", reservation_id))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_reservation_edit(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("res_edit:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in edit button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Check permission - user must own the reservation or be admin
        let user_id = interaction.user.id.get() as i64;
        let guild_id = interaction.guild_id.unwrap();
        
        let reservation = sqlx::query!(
            "SELECT id, equipment_id, user_id, start_time, end_time, location 
             FROM reservations WHERE id = ? AND status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found or already cancelled.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        let is_owner = reservation.user_id == user_id;
        let is_admin = utils::is_admin(ctx, guild_id, interaction.user.id).await?;

        if !is_owner && !is_admin {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You can only edit your own reservations.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Get equipment name for modal title
        let equipment = sqlx::query!(
            "SELECT name FROM equipment WHERE id = ?",
            reservation.equipment_id
        )
        .fetch_one(&self.db)
        .await?;

        // Pre-fill modal with current values
        use crate::time;
        let start_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
        let end_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));

        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("edit_reservation_modal:{}", reservation_id), 
            format!("Edit Reservation - {}", equipment.name)
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "start_time", "Start Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .value(start_jst)
                    .required(true),
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "end_time", "End Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .value(end_jst)
                    .required(true),
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "location", "Return Location (Optional)")
                    .value(reservation.location.unwrap_or_default())
                    .required(false),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_edit_reservation_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("edit_reservation_modal:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in edit modal: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Extract modal data
        let mut start_time_str = String::new();
        let mut end_time_str = String::new();
        let mut location = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    match input_text.custom_id.as_str() {
                        "start_time" => start_time_str = input_text.value.clone().unwrap_or_default(),
                        "end_time" => end_time_str = input_text.value.clone().unwrap_or_default(),
                        "location" => location = input_text.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        // Parse and validate times
        let (start_utc, end_utc) = match self.parse_and_validate_times(&start_time_str, &end_time_str) {
            Ok(times) => times,
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Update reservation with conflict detection
        match self.update_reservation_with_conflict_check(
            reservation_id,
            start_utc,
            end_utc,
            if location.is_empty() { None } else { Some(location) },
        ).await {
            Ok(_) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("✅ Reservation updated successfully!")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_reservation_cancel(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("res_cancel:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in cancel button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Check permission and get reservation details
        let user_id = interaction.user.id.get() as i64;
        let guild_id = interaction.guild_id.unwrap();
        
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found or already cancelled.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        let is_owner = reservation.user_id == user_id;
        let is_admin = utils::is_admin(ctx, guild_id, interaction.user.id).await?;

        if !is_owner && !is_admin {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You can only cancel your own reservations.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Cancel the reservation
        match self.cancel_reservation(reservation_id, user_id).await {
            Ok(_) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                use crate::time;
                let start_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
                let end_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));

                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!(
                            "✅ Reservation cancelled successfully!\n\n**Equipment:** {}\n**Period:** {} to {}",
                            reservation.equipment_name, start_jst, end_jst
                        ))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_reservation_admin_cancel(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Check admin permissions first
        if !utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await? {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You need administrator permissions to use this feature.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("res_admin_cancel:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in admin cancel button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Get reservation details
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found or already cancelled.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Cancel the reservation (admin action)
        let admin_id = interaction.user.id.get() as i64;
        match self.cancel_reservation(reservation_id, admin_id).await {
            Ok(_) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                use crate::time;
                let start_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
                let end_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));

                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!(
                            "✅ Reservation cancelled by admin!\n\n**Equipment:** {}\n**Original User:** <@{}>\n**Period:** {} to {}",
                            reservation.equipment_name, reservation.user_id, start_jst, end_jst
                        ))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    // Helper methods for reservation management

    fn naive_datetime_to_utc(naive: chrono::NaiveDateTime) -> chrono::DateTime<chrono::Utc> {
        chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
    }

    fn parse_and_validate_times(&self, start_str: &str, end_str: &str) -> Result<(chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>), String> {
        use crate::time;

        // Parse start time
        let start_parts: Vec<&str> = start_str.split(&[' ', ':', '-'][..]).collect();
        if start_parts.len() != 5 {
            return Err("Invalid start time format. Use YYYY-MM-DD HH:MM".to_string());
        }

        let start_year: i32 = start_parts[0].parse().map_err(|_| "Invalid start year")?;
        let start_month: u32 = start_parts[1].parse().map_err(|_| "Invalid start month")?;
        let start_day: u32 = start_parts[2].parse().map_err(|_| "Invalid start day")?;
        let start_hour: u32 = start_parts[3].parse().map_err(|_| "Invalid start hour")?;
        let start_minute: u32 = start_parts[4].parse().map_err(|_| "Invalid start minute")?;

        // Parse end time
        let end_parts: Vec<&str> = end_str.split(&[' ', ':', '-'][..]).collect();
        if end_parts.len() != 5 {
            return Err("Invalid end time format. Use YYYY-MM-DD HH:MM".to_string());
        }

        let end_year: i32 = end_parts[0].parse().map_err(|_| "Invalid end year")?;
        let end_month: u32 = end_parts[1].parse().map_err(|_| "Invalid end month")?;
        let end_day: u32 = end_parts[2].parse().map_err(|_| "Invalid end day")?;
        let end_hour: u32 = end_parts[3].parse().map_err(|_| "Invalid end hour")?;
        let end_minute: u32 = end_parts[4].parse().map_err(|_| "Invalid end minute")?;

        // Convert JST to UTC
        let start_utc = time::jst_to_utc(start_year, start_month, start_day, start_hour, start_minute)
            .ok_or("Invalid start date/time")?;
        let end_utc = time::jst_to_utc(end_year, end_month, end_day, end_hour, end_minute)
            .ok_or("Invalid end date/time")?;

        // Validate times
        if end_utc <= start_utc {
            return Err("End time must be after start time".to_string());
        }

        let now = chrono::Utc::now();
        if start_utc < now {
            return Err("Start time cannot be in the past".to_string());
        }

        // Max 60 days in the future
        let max_future = now + chrono::Duration::days(60);
        if end_utc > max_future {
            return Err("Reservation cannot extend more than 60 days into the future".to_string());
        }

        Ok((start_utc, end_utc))
    }

    async fn create_reservation_with_conflict_check(
        &self,
        equipment_id: i64,
        user_id: i64,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
        location: Option<String>,
    ) -> Result<i64, String> {
        // Start transaction for conflict detection
        let mut tx = self.db.begin().await.map_err(|e| format!("Database error: {}", e))?;

        // Check for conflicts
        let conflicts = sqlx::query!(
            "SELECT id, user_id, start_time, end_time FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' 
             AND start_time < ? AND end_time > ?",
            equipment_id,
            end_time,
            start_time
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        if !conflicts.is_empty() {
            let conflict = &conflicts[0];
            let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.start_time));
            let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.end_time));
            return Err(format!(
                "Reservation conflicts with existing booking by <@{}> from {} to {}",
                conflict.user_id, start_jst, end_jst
            ));
        }

        // Create reservation
        let result = sqlx::query!(
            "INSERT INTO reservations (equipment_id, user_id, start_time, end_time, location, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, 'Confirmed', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
            equipment_id,
            user_id,
            start_time,
            end_time,
            location
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to create reservation: {}", e))?;

        let reservation_id = result.last_insert_rowid();

        // Log the reservation
        let log_notes = format!("Reservation ID: {}", reservation_id);
        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'Reserved', ?, NULL, 'Confirmed', ?, CURRENT_TIMESTAMP)",
            equipment_id,
            user_id,
            location,
            log_notes
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to log reservation: {}", e))?;

        tx.commit().await.map_err(|e| format!("Failed to commit transaction: {}", e))?;

        Ok(reservation_id)
    }

    async fn update_reservation_with_conflict_check(
        &self,
        reservation_id: i64,
        start_time: chrono::DateTime<chrono::Utc>,
        end_time: chrono::DateTime<chrono::Utc>,
        location: Option<String>,
    ) -> Result<(), String> {
        // Start transaction for conflict detection
        let mut tx = self.db.begin().await.map_err(|e| format!("Database error: {}", e))?;

        // Get current reservation details
        let current = sqlx::query!(
            "SELECT equipment_id, user_id, start_time, end_time, location FROM reservations WHERE id = ?",
            reservation_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Reservation not found")?;

        // Check for conflicts (excluding this reservation)
        let conflicts = sqlx::query!(
            "SELECT id, user_id, start_time, end_time FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' AND id != ?
             AND start_time < ? AND end_time > ?",
            current.equipment_id,
            reservation_id,
            end_time,
            start_time
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        if !conflicts.is_empty() {
            let conflict = &conflicts[0];
            let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.start_time));
            let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.end_time));
            return Err(format!(
                "Updated reservation would conflict with existing booking by <@{}> from {} to {}",
                conflict.user_id, start_jst, end_jst
            ));
        }

        // Update reservation
        sqlx::query!(
            "UPDATE reservations SET start_time = ?, end_time = ?, location = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            start_time,
            end_time,
            location,
            reservation_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update reservation: {}", e))?;

        // Create change notes
        let mut notes = Vec::new();
        if Self::naive_datetime_to_utc(current.start_time) != start_time {
            let old_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(current.start_time));
            let new_jst = crate::time::utc_to_jst_string(start_time);
            notes.push(format!("Start: {} → {}", old_jst, new_jst));
        }
        if Self::naive_datetime_to_utc(current.end_time) != end_time {
            let old_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(current.end_time));
            let new_jst = crate::time::utc_to_jst_string(end_time);
            notes.push(format!("End: {} → {}", old_jst, new_jst));
        }
        if current.location != location {
            let old_loc = current.location.unwrap_or("None".to_string());
            let new_loc = location.clone().unwrap_or("None".to_string());
            notes.push(format!("Location: {} → {}", old_loc, new_loc));
        }

        // Log the update
        let log_notes = if notes.is_empty() {
            "No changes".to_string()
        } else {
            format!("Reservation ID: {} - {}", reservation_id, notes.join(", "))
        };

        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'Edited', ?, 'Confirmed', 'Confirmed', ?, CURRENT_TIMESTAMP)",
            current.equipment_id,
            current.user_id,
            location,
            log_notes
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to log reservation update: {}", e))?;

        tx.commit().await.map_err(|e| format!("Failed to commit transaction: {}", e))?;

        Ok(())
    }

    async fn cancel_reservation(&self, reservation_id: i64, cancelling_user_id: i64) -> Result<(), String> {
        // Start transaction
        let mut tx = self.db.begin().await.map_err(|e| format!("Database error: {}", e))?;

        // Get reservation details
        let reservation = sqlx::query!(
            "SELECT equipment_id, user_id FROM reservations WHERE id = ? AND status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Reservation not found or already cancelled")?;

        // Cancel the reservation
        sqlx::query!(
            "UPDATE reservations SET status = 'Cancelled', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            reservation_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to cancel reservation: {}", e))?;

        // Log the cancellation
        let is_self_cancel = reservation.user_id == cancelling_user_id;
        let notes = if is_self_cancel {
            format!("Reservation ID: {} - Cancelled by owner", reservation_id)
        } else {
            format!("Reservation ID: {} - Cancelled by admin <@{}>", reservation_id, cancelling_user_id)
        };

        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'Cancelled', NULL, 'Confirmed', 'Cancelled', ?, CURRENT_TIMESTAMP)",
            reservation.equipment_id,
            cancelling_user_id,
            notes
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to log reservation cancellation: {}", e))?;

        tx.commit().await.map_err(|e| format!("Failed to commit transaction: {}", e))?;

        Ok(())
    }

    // Reservation wizard button handlers

    async fn handle_reservation_wizard_start_input(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("reserve_start_time_modal:{}", interaction.token), 
            "Enter Start Time"
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "start_time", "Start Date & Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .required(true),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_reservation_wizard_end_input(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("reserve_end_time_modal:{}", interaction.token), 
            "Enter End Time"
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "end_time", "End Date & Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .required(true),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_reservation_wizard_location_input(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("reserve_location_modal:{}", interaction.token), 
            "Enter Return Location"
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "location", "Return Location")
                    .placeholder("Where will you return this equipment?")
                    .required(true),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_reservation_wizard_location_default(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Get equipment default location and update state
        let (equipment_name, start_time, end_time, default_location) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                let equipment = sqlx::query!(
                    "SELECT name, default_return_location FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => {
                        state.location = eq.default_return_location.clone();
                        state.step = WizardStep::Confirmation;
                        (eq.name, state.start_time, state.end_time, eq.default_return_location)
                    }
                    None => {
                        return self.handle_reservation_wizard_cancel(ctx, interaction).await;
                    }
                }
            } else {
                return self.handle_reservation_wizard_cancel(ctx, interaction).await;
            }
        };

        if let (Some(start), Some(end)) = (start_time, end_time) {
            self.show_confirmation_step(ctx, interaction, &equipment_name, start, end, default_location).await?;
        } else {
            self.handle_reservation_wizard_cancel(ctx, interaction).await?;
        }

        Ok(())
    }

    async fn handle_reservation_wizard_location_skip(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Update state to skip location
        let (equipment_name, start_time, end_time) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.location = None;
                state.step = WizardStep::Confirmation;
                
                let equipment = sqlx::query!(
                    "SELECT name FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => (eq.name, state.start_time, state.end_time),
                    None => {
                        return self.handle_reservation_wizard_cancel(ctx, interaction).await;
                    }
                }
            } else {
                return self.handle_reservation_wizard_cancel(ctx, interaction).await;
            }
        };

        if let (Some(start), Some(end)) = (start_time, end_time) {
            self.show_confirmation_step(ctx, interaction, &equipment_name, start, end, None).await?;
        } else {
            self.handle_reservation_wizard_cancel(ctx, interaction).await?;
        }

        Ok(())
    }

    async fn handle_reservation_wizard_back_start(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Reset to start time step
        let equipment_name = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.step = WizardStep::StartTime;
                state.start_time = None;
                state.end_time = None;
                
                let equipment = sqlx::query!(
                    "SELECT name FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => eq.name,
                    None => {
                        return self.handle_reservation_wizard_cancel(ctx, interaction).await;
                    }
                }
            } else {
                return self.handle_reservation_wizard_cancel(ctx, interaction).await;
            }
        };

        self.show_start_time_step(ctx, interaction, &equipment_name).await?;
        Ok(())
    }

    async fn handle_reservation_wizard_back_end(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Reset to end time step
        let (equipment_name, start_time) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.step = WizardStep::EndTime;
                state.end_time = None;
                
                let equipment = sqlx::query!(
                    "SELECT name FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => (eq.name, state.start_time),
                    None => {
                        return self.handle_reservation_wizard_cancel(ctx, interaction).await;
                    }
                }
            } else {
                return self.handle_reservation_wizard_cancel(ctx, interaction).await;
            }
        };

        if let Some(start) = start_time {
            self.show_end_time_step(ctx, interaction, &equipment_name, start).await?;
        } else {
            self.show_start_time_step(ctx, interaction, &equipment_name).await?;
        }
        
        Ok(())
    }

    async fn handle_reservation_wizard_back_location(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Reset to location step
        let (equipment_name, start_time, end_time, default_location) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.step = WizardStep::Location;
                
                let equipment = sqlx::query!(
                    "SELECT name, default_return_location FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => (eq.name, state.start_time, state.end_time, eq.default_return_location),
                    None => {
                        return self.handle_reservation_wizard_cancel(ctx, interaction).await;
                    }
                }
            } else {
                return self.handle_reservation_wizard_cancel(ctx, interaction).await;
            }
        };

        if let (Some(start), Some(end)) = (start_time, end_time) {
            self.show_location_step(ctx, interaction, &equipment_name, start, end, default_location).await?;
        } else {
            self.show_start_time_step(ctx, interaction, &equipment_name).await?;
        }
        
        Ok(())
    }

    async fn handle_reservation_wizard_confirm(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Get final state and create reservation
        let (equipment_id, user_id, start_time, end_time, location) = {
            let states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get(&state_key) {
                (
                    state.equipment_id,
                    state.user_id.get() as i64,
                    state.start_time,
                    state.end_time,
                    state.location.clone(),
                )
            } else {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Session expired. Please start the reservation process again.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        if let (Some(start), Some(end)) = (start_time, end_time) {
            // Create reservation with conflict detection
            match self.create_reservation_with_conflict_check(
                equipment_id,
                user_id,
                start,
                end,
                location,
            ).await {
                Ok(reservation_id) => {
                    // Success - refresh equipment display
                    if let Some(guild_id) = interaction.guild_id {
                        let guild_id_i64 = guild_id.get() as i64;
                        if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                            let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                            let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                        }
                    }

                    let start_jst = crate::time::utc_to_jst_string(start);
                    let end_jst = crate::time::utc_to_jst_string(end);

                    let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                        serenity::all::CreateInteractionResponseMessage::new()
                            .content(format!("✅ **Reservation Created Successfully!**\n\n🆔 **Reservation ID:** {}\n📅 **Period:** {} to {} (JST)\n\nYour equipment reservation is now confirmed!", reservation_id, start_jst, end_jst))
                            .components(vec![]),
                    );
                    interaction.create_response(&ctx.http, response).await?;
                }
                Err(err_msg) => {
                    let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                        serenity::all::CreateInteractionResponseMessage::new()
                            .content(format!("❌ **Failed to Create Reservation**\n\n{}", err_msg))
                            .components(vec![]),
                    );
                    interaction.create_response(&ctx.http, response).await?;
                }
            }
        } else {
            let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Invalid reservation state. Please start again.")
                    .components(vec![]),
            );
            interaction.create_response(&ctx.http, response).await?;
        }

        // Clean up wizard state
        {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            states.remove(&state_key);
        }

        Ok(())
    }

    async fn handle_reservation_wizard_cancel(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let state_key = (interaction.user.id, interaction.token.clone());
        
        // Clean up wizard state
        {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            states.remove(&state_key);
        }

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("❌ **Reservation Cancelled**\n\nThe reservation process has been cancelled. You can start a new reservation anytime by clicking the Reserve button on any available equipment.")
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    // Wizard modal handlers

    async fn handle_reservation_wizard_start_time_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let token = interaction.data.custom_id
            .strip_prefix("reserve_start_time_modal:")
            .unwrap_or("");
        
        let state_key = (interaction.user.id, token.to_string());
        
        // Extract start time from modal
        let mut start_time_str = String::new();
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    if input_text.custom_id == "start_time" {
                        start_time_str = input_text.value.clone().unwrap_or_default();
                        break;
                    }
                }
            }
        }

        // Parse and validate start time using new parse_jst_string function
        let start_utc = match crate::time::parse_jst_string(&start_time_str) {
            Some(time) => time,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Invalid start time format. Please use YYYY-MM-DD HH:MM (JST).")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Validate start time is in the future
        let now = chrono::Utc::now();
        if start_utc < now {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Start time cannot be in the past. Please choose a future time.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Update wizard state and proceed to end time step
        let (equipment_name, success) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.start_time = Some(start_utc);
                state.step = WizardStep::EndTime;
                
                let equipment = sqlx::query!(
                    "SELECT name FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => (eq.name, true),
                    None => (String::new(), false)
                }
            } else {
                (String::new(), false)
            }
        };

        if success {
            // Simulate a component interaction for the next step
            let fake_interaction = ComponentInteractionRef {
                user: interaction.user.clone(),
                token: token.to_string(),
                guild_id: interaction.guild_id,
                channel_id: interaction.channel_id,
            };
            
            self.show_end_time_step_from_modal(ctx, &fake_interaction, &equipment_name, start_utc).await?;
        } else {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Session expired. Please start the reservation process again.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
        }

        Ok(())
    }

    async fn handle_reservation_wizard_end_time_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let token = interaction.data.custom_id
            .strip_prefix("reserve_end_time_modal:")
            .unwrap_or("");
        
        let state_key = (interaction.user.id, token.to_string());
        
        // Extract end time from modal
        let mut end_time_str = String::new();
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    if input_text.custom_id == "end_time" {
                        end_time_str = input_text.value.clone().unwrap_or_default();
                        break;
                    }
                }
            }
        }

        // Parse and validate end time
        let end_utc = match crate::time::parse_jst_string(&end_time_str) {
            Some(time) => time,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Invalid end time format. Please use YYYY-MM-DD HH:MM (JST).")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Update wizard state and validate against start time
        let (equipment_name, start_time, default_location, success) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                if let Some(start) = state.start_time {
                    // Validate end time is after start time
                    if end_utc <= start {
                        let response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content("❌ End time must be after start time.")
                                .ephemeral(true),
                        );
                        interaction.create_response(&ctx.http, response).await?;
                        return Ok(());
                    }

                    // Validate max 60 days duration
                    let max_future = chrono::Utc::now() + chrono::Duration::days(60);
                    if end_utc > max_future {
                        let response = serenity::all::CreateInteractionResponse::Message(
                            serenity::all::CreateInteractionResponseMessage::new()
                                .content("❌ Reservation cannot extend more than 60 days into the future.")
                                .ephemeral(true),
                        );
                        interaction.create_response(&ctx.http, response).await?;
                        return Ok(());
                    }

                    state.end_time = Some(end_utc);
                    state.step = WizardStep::Location;
                    
                    let equipment = sqlx::query!(
                        "SELECT name, default_return_location FROM equipment WHERE id = ?",
                        state.equipment_id
                    )
                    .fetch_optional(&self.db)
                    .await?;

                    match equipment {
                        Some(eq) => (eq.name, start, eq.default_return_location, true),
                        None => (String::new(), start, None, false)
                    }
                } else {
                    (String::new(), chrono::Utc::now(), None, false)
                }
            } else {
                (String::new(), chrono::Utc::now(), None, false)
            }
        };

        if success {
            // Simulate a component interaction for the next step
            let fake_interaction = ComponentInteractionRef {
                user: interaction.user.clone(),
                token: token.to_string(),
                guild_id: interaction.guild_id,
                channel_id: interaction.channel_id,
            };
            
            self.show_location_step_from_modal(ctx, &fake_interaction, &equipment_name, start_time, end_utc, default_location).await?;
        } else {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Session expired. Please start the reservation process again.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
        }

        Ok(())
    }

    async fn handle_reservation_wizard_location_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let token = interaction.data.custom_id
            .strip_prefix("reserve_location_modal:")
            .unwrap_or("");
        
        let state_key = (interaction.user.id, token.to_string());
        
        // Extract location from modal
        let mut location = String::new();
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    if input_text.custom_id == "location" {
                        location = input_text.value.clone().unwrap_or_default();
                        break;
                    }
                }
            }
        }

        // Update wizard state and proceed to confirmation
        let (equipment_name, start_time, end_time, success) = {
            let mut states = RESERVATION_WIZARD_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.location = if location.is_empty() { None } else { Some(location.clone()) };
                state.step = WizardStep::Confirmation;
                
                let equipment = sqlx::query!(
                    "SELECT name FROM equipment WHERE id = ?",
                    state.equipment_id
                )
                .fetch_optional(&self.db)
                .await?;

                match equipment {
                    Some(eq) => (eq.name, state.start_time, state.end_time, true),
                    None => (String::new(), None, None, false)
                }
            } else {
                (String::new(), None, None, false)
            }
        };

        if success && start_time.is_some() && end_time.is_some() {
            // Simulate a component interaction for the next step
            let fake_interaction = ComponentInteractionRef {
                user: interaction.user.clone(),
                token: token.to_string(),
                guild_id: interaction.guild_id,
                channel_id: interaction.channel_id,
            };
            
            let location_opt = if location.is_empty() { None } else { Some(location) };
            self.show_confirmation_step_from_modal(ctx, &fake_interaction, &equipment_name, start_time.unwrap(), end_time.unwrap(), location_opt).await?;
        } else {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Session expired. Please start the reservation process again.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
        }

        Ok(())
    }

    // Helper methods for modal-triggered step displays
    async fn show_end_time_step_from_modal(
        &self,
        ctx: &Context,
        interaction: &ComponentInteractionRef,
        equipment_name: &str,
        start_time: DateTime<Utc>,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour, EditMessage};
        
        let start_jst = crate::time::utc_to_jst_string(start_time);
        
        let embed = CreateEmbed::new()
            .title("📅 Reserve Equipment - Step 2/3")
            .description(format!("**Equipment:** {}\n**Start Time:** {}\n\n**Step 2:** Please enter the end date and time for your reservation.\n\n⏰ **Format:** YYYY-MM-DD HH:MM (JST)\n📝 **Example:** 2024-01-15 18:30\n\n⚠️ **Note:** End time must be after start time and within 60 days.", equipment_name, start_jst))
            .color(Colour::BLUE)
            .footer(serenity::all::CreateEmbedFooter::new("Times are in Japan Standard Time (JST)"));

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("reserve_end_input:{}", interaction.token))
                .label("📅 Enter End Time")
                .style(ButtonStyle::Primary),
            CreateButton::new(format!("reserve_back_start:{}", interaction.token))
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        // For modals, we need to edit the original interaction message
        let edit = EditMessage::new()
            .embed(embed)
            .components(vec![buttons]);

        ctx.http.edit_original_interaction_response(&interaction.token, &edit, Vec::new()).await?;
        Ok(())
    }

    async fn show_location_step_from_modal(
        &self,
        ctx: &Context,
        interaction: &ComponentInteractionRef,
        equipment_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        default_location: Option<String>,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour, EditMessage};
        
        let start_jst = crate::time::utc_to_jst_string(start_time);
        let end_jst = crate::time::utc_to_jst_string(end_time);
        
        let embed = CreateEmbed::new()
            .title("📍 Reserve Equipment - Step 3/3")
            .description(format!("**Equipment:** {}\n**Start Time:** {}\n**End Time:** {}\n\n**Step 3:** Please specify the return location (optional).\n\n📍 You can use the default location or enter a custom one.", equipment_name, start_jst, end_jst))
            .color(Colour::BLUE);

        let mut buttons = vec![
            CreateButton::new(format!("reserve_location_input:{}", interaction.token))
                .label("📍 Enter Location")
                .style(ButtonStyle::Primary),
        ];

        if let Some(ref default_loc) = default_location {
            if !default_loc.is_empty() {
                buttons.push(
                    CreateButton::new(format!("reserve_location_default:{}", interaction.token))
                        .label(format!("📍 Use Default ({})", default_loc))
                        .style(ButtonStyle::Secondary)
                );
            }
        }

        buttons.extend_from_slice(&[
            CreateButton::new(format!("reserve_location_skip:{}", interaction.token))
                .label("⏭️ Skip Location")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_back_end:{}", interaction.token))
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let edit = EditMessage::new()
            .embed(embed)
            .components(vec![CreateActionRow::Buttons(buttons)]);

        ctx.http.edit_original_interaction_response(&interaction.token, &edit, Vec::new()).await?;
        Ok(())
    }

    async fn show_confirmation_step_from_modal(
        &self,
        ctx: &Context,
        interaction: &ComponentInteractionRef,
        equipment_name: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        location: Option<String>,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour, EditMessage};
        
        let start_jst = crate::time::utc_to_jst_string(start_time);
        let end_jst = crate::time::utc_to_jst_string(end_time);
        let location_text = location.as_deref().unwrap_or("Not specified");
        
        // Check for conflicts in real-time before showing confirmation
        let state_key = (interaction.user.id, interaction.token.to_string());
        let equipment_id = {
            let states = RESERVATION_WIZARD_STATES.lock().await;
            states.get(&state_key).map(|s| s.equipment_id).unwrap_or(0)
        };

        if equipment_id == 0 {
            let edit = EditMessage::new()
                .content("❌ Session expired. Please start the reservation process again.")
                .components(vec![]);
            ctx.http.edit_original_interaction_response(&interaction.token, &edit, Vec::new()).await?;
            return Ok(());
        }

        // Check for conflicts
        let conflicts = sqlx::query!(
            "SELECT id, user_id, start_time, end_time FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' 
             AND start_time < ? AND end_time > ?",
            equipment_id,
            end_time,
            start_time
        )
        .fetch_all(&self.db)
        .await?;

        if !conflicts.is_empty() {
            let conflict = &conflicts[0];
            let conflict_start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.start_time));
            let conflict_end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(conflict.end_time));
            
            let embed = CreateEmbed::new()
                .title("⚠️ Reservation Conflict Detected")
                .description(format!("**Equipment:** {}\n\n❌ **Conflict:** Your requested time overlaps with an existing reservation by <@{}> from {} to {}.\n\nPlease go back and choose different times.", equipment_name, conflict.user_id, conflict_start_jst, conflict_end_jst))
                .color(Colour::RED);

            let buttons = CreateActionRow::Buttons(vec![
                CreateButton::new(format!("reserve_back_location:{}", interaction.token))
                    .label("⬅️ Back to Times")
                    .style(ButtonStyle::Secondary),
                CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                    .label("❌ Cancel")
                    .style(ButtonStyle::Danger),
            ]);

            let edit = EditMessage::new()
                .embed(embed)
                .components(vec![buttons]);

            ctx.http.edit_original_interaction_response(&interaction.token, &edit, Vec::new()).await?;
            return Ok(());
        }
        
        let embed = CreateEmbed::new()
            .title("✅ Confirm Reservation")
            .description(format!("**Equipment:** {}\n**Start Time:** {}\n**End Time:** {}\n**Return Location:** {}\n\n🔍 **Conflict Check:** ✅ No conflicts detected\n\nPlease confirm your reservation details.", equipment_name, start_jst, end_jst, location_text))
            .color(Colour::DARK_GREEN);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("reserve_confirm:{}", interaction.token))
                .label("✅ Confirm Reservation")
                .style(ButtonStyle::Success),
            CreateButton::new(format!("reserve_back_location:{}", interaction.token))
                .label("⬅️ Back")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("reserve_cancel:{}", interaction.token))
                .label("❌ Cancel")
                .style(ButtonStyle::Danger),
        ]);

        let edit = EditMessage::new()
            .embed(embed)
            .components(vec![buttons]);

        ctx.http.edit_original_interaction_response(&interaction.token, &edit, Vec::new()).await?;
        Ok(())
    }

    // Change reservation handlers

    async fn handle_change_reservation_select(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Extract the selected reservation ID
        let reservation_id_str = if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
            values.first()
                .and_then(|v| v.strip_prefix("reservation_"))
                .unwrap_or("")
        } else {
            ""
        };
        
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in select: {:?}", interaction.data.kind);
            return Ok(());
        }

        // Get reservation details
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, r.location, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found or has been cancelled.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Verify ownership (allow admin override)
        let user_id = interaction.user.id.get() as i64;
        let is_owner = reservation.user_id == user_id;
        let is_admin = utils::is_admin(ctx, interaction.guild_id.unwrap(), interaction.user.id).await?;

        if !is_owner && !is_admin {
            let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You can only manage your own reservations.")
                    .components(vec![]),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Show management options
        let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
        let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));
        let location_text = reservation.location.as_deref().unwrap_or("Not specified");

        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let embed = CreateEmbed::new()
            .title("🔧 Manage Reservation")
            .description(format!("**Equipment:** {}\n**Period:** {} to {}\n**Location:** {}\n\nWhat would you like to do?", 
                reservation.equipment_name, start_jst, end_jst, location_text))
            .color(Colour::BLUE);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("change_res_time:{}", reservation_id))
                .label("📅 Change Time")
                .style(ButtonStyle::Primary),
            CreateButton::new(format!("change_res_location:{}", reservation_id))
                .label("📍 Change Location")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("cancel_res:{}", reservation_id))
                .label("❌ Cancel Reservation")
                .style(ButtonStyle::Danger),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_change_reservation_time(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("change_res_time:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in change time button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Get reservation details for pre-filling
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, r.location, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Pre-fill modal with current values
        let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
        let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));

        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("change_time_modal:{}", reservation_id), 
            format!("Change Reservation Time - {}", reservation.equipment_name)
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "start_time", "New Start Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .value(start_jst)
                    .required(true),
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "end_time", "New End Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .value(end_jst)
                    .required(true),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_change_reservation_location(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("change_res_location:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in change location button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Get current location for pre-filling
        let reservation = sqlx::query!(
            "SELECT r.location, e.name as equipment_name 
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("change_location_modal:{}", reservation_id), 
            format!("Change Return Location - {}", reservation.equipment_name)
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "location", "New Return Location")
                    .placeholder("Leave empty to remove location")
                    .value(reservation.location.unwrap_or_default())
                    .required(false),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_cancel_reservation_confirm(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("cancel_res:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in cancel button: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Get reservation details for confirmation
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found or already cancelled.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Show confirmation dialog
        let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
        let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));

        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let embed = CreateEmbed::new()
            .title("⚠️ Cancel Reservation")
            .description(format!("**Equipment:** {}\n**Period:** {} to {}\n\n❌ **Warning:** This action cannot be undone!\n\nAre you sure you want to cancel this reservation?", 
                reservation.equipment_name, start_jst, end_jst))
            .color(Colour::RED);

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("confirm_cancel_res:{}", reservation_id))
                .label("❌ Yes, Cancel")
                .style(ButtonStyle::Danger),
            CreateButton::new(format!("abort_cancel_res:{}", reservation_id))
                .label("↩️ No, Go Back")
                .style(ButtonStyle::Secondary),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .embed(embed)
                .components(vec![buttons]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_confirm_cancel_reservation(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("confirm_cancel_res:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in confirm cancel: {}", interaction.data.custom_id);
            return Ok(());
        }

        let user_id = interaction.user.id.get() as i64;
        
        // Cancel the reservation
        match self.cancel_reservation(reservation_id, user_id).await {
            Ok(_) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("✅ **Reservation Cancelled Successfully!**\n\nYour reservation has been cancelled.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ **Failed to Cancel Reservation**\n\n{}", err_msg))
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_abort_cancel_reservation(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("↩️ **Cancellation Aborted**\n\nYour reservation remains active.")
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_change_time_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("change_time_modal:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in change time modal: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Extract modal data
        let mut start_time_str = String::new();
        let mut end_time_str = String::new();

        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    match input_text.custom_id.as_str() {
                        "start_time" => start_time_str = input_text.value.clone().unwrap_or_default(),
                        "end_time" => end_time_str = input_text.value.clone().unwrap_or_default(),
                        _ => {}
                    }
                }
            }
        }

        // Parse and validate times
        let (start_utc, end_utc) = match self.parse_and_validate_times(&start_time_str, &end_time_str) {
            Ok(times) => times,
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ {}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Get current location
        let current_location = sqlx::query_scalar!(
            "SELECT location FROM reservations WHERE id = ?",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?
        .flatten();

        // Update reservation with conflict detection
        match self.update_reservation_with_conflict_check(
            reservation_id,
            start_utc,
            end_utc,
            current_location,
        ).await {
            Ok(_) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                let start_jst = crate::time::utc_to_jst_string(start_utc);
                let end_jst = crate::time::utc_to_jst_string(end_utc);

                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("✅ **Reservation Time Updated!**\n\n📅 **New Period:** {} to {} (JST)", start_jst, end_jst))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ **Failed to Update Reservation**\n\n{}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_change_location_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("change_location_modal:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in change location modal: {}", interaction.data.custom_id);
            return Ok(());
        }

        // Extract location from modal
        let mut location = String::new();
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    if input_text.custom_id == "location" {
                        location = input_text.value.clone().unwrap_or_default();
                        break;
                    }
                }
            }
        }

        let location_opt = if location.is_empty() { None } else { Some(location.clone()) };

        // Get current times
        let current = sqlx::query!(
            "SELECT start_time, end_time FROM reservations WHERE id = ?",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let current = match current {
            Some(res) => res,
            None => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Reservation not found.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        let start_utc = Self::naive_datetime_to_utc(current.start_time);
        let end_utc = Self::naive_datetime_to_utc(current.end_time);

        // Update reservation location
        match self.update_reservation_with_conflict_check(
            reservation_id,
            start_utc,
            end_utc,
            location_opt.clone(),
        ).await {
            Ok(_) => {
                // Success - refresh equipment display
                if let Some(guild_id) = interaction.guild_id {
                    let guild_id_i64 = guild_id.get() as i64;
                    if let Ok(channel_id) = self.get_reservation_channel_id(guild_id_i64).await {
                        let renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                        let _ = renderer.reconcile_equipment_display(ctx, guild_id_i64, channel_id).await;
                    }
                }

                let location_text = location_opt.as_deref().unwrap_or("Not specified");
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("✅ **Reservation Location Updated!**\n\n📍 **New Location:** {}", location_text))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ **Failed to Update Location**\n\n{}", err_msg))
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }
}
