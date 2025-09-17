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
                SetupCommand::handle_confirmation(ctx, interaction, &self.db, false).await?
            }
            "overall_management" => {
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
                error!(
                    "Unknown component interaction: {}",
                    interaction.data.custom_id
                );
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
                error!("Unknown modal interaction: {}", interaction.data.custom_id);
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
}
