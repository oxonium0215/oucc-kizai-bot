use anyhow::Result;
use serenity::async_trait;
use serenity::model::prelude::*;
use serenity::prelude::*;
use sqlx::SqlitePool;
use tracing::{error, info};

use crate::commands::SetupCommand;
use crate::utils;
use crate::equipment::EquipmentRenderer;

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
                // Check for dynamic reservation and equipment IDs
                if interaction.data.custom_id.starts_with("eq_reserve:") {
                    self.handle_equipment_reserve(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("res_edit:") {
                    self.handle_reservation_edit(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("res_cancel:") {
                    self.handle_reservation_cancel(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("res_admin_cancel:") {
                    self.handle_reservation_admin_cancel(ctx, interaction).await?
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
        let equipment_id_str = interaction.data.custom_id
            .strip_prefix("eq_reserve:")
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

        // Create reservation modal
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        let modal = CreateModal::new(
            format!("reserve_modal:{}", equipment_id), 
            format!("Reserve {}", equipment.name)
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "start_time", "Start Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .required(true),
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "end_time", "End Time")
                    .placeholder("YYYY-MM-DD HH:MM (JST)")
                    .required(true),
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "location", "Return Location (Optional)")
                    .placeholder(&equipment.default_return_location.unwrap_or_default())
                    .required(false),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
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

    async fn get_reservation_channel_id(&self, guild_id: i64) -> Result<i64, sqlx::Error> {
        let channel_id = sqlx::query_scalar!(
            "SELECT reservation_channel_id FROM guilds WHERE id = ?",
            guild_id
        )
        .fetch_one(&self.db)
        .await?;

        Ok(channel_id.unwrap_or(0))
    }
}
