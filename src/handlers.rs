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

// Overall Management state
#[derive(Debug, Clone)]
pub struct ManagementState {
    equipment_filter: Option<Vec<i64>>, // Equipment IDs, None means all
    time_filter: TimeFilter,
    status_filter: StatusFilter,
    page: usize,
    items_per_page: usize,
}

#[derive(Debug, Clone)]
pub enum TimeFilter {
    Today,
    Next24h,
    Next7days,
    Custom { start_utc: DateTime<Utc>, end_utc: DateTime<Utc> },
    All,
}

#[derive(Debug, Clone)]
pub enum StatusFilter {
    Active,      // Currently loaned
    Upcoming,    // Future reservations
    ReturnedToday, // Returned today
    All,
}

impl Default for ManagementState {
    fn default() -> Self {
        Self {
            equipment_filter: None,
            time_filter: TimeFilter::All,
            status_filter: StatusFilter::All,
            page: 0,
            items_per_page: 10,
        }
    }
}

lazy_static::lazy_static! {
    static ref RESERVATION_WIZARD_STATES: Arc<Mutex<HashMap<(UserId, String), ReservationWizardState>>> = Arc::new(Mutex::new(HashMap::new()));
    static ref MANAGEMENT_STATES: Arc<Mutex<HashMap<(GuildId, UserId, String), ManagementState>>> = Arc::new(Mutex::new(HashMap::new()));
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
            "notification_preferences" => {
                SetupCommand::handle_notification_preferences(ctx, interaction, &self.db).await?
            }
            "notification_next" => {
                SetupCommand::handle_notification_next(ctx, interaction, &self.db).await?
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
                } else if interaction.data.custom_id.starts_with("return_select:") {
                    self.handle_return_select(ctx, interaction).await?
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
                } else if interaction.data.custom_id.starts_with("confirm_return:") {
                    self.handle_confirm_return(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("cancel_return:") {
                    self.handle_cancel_return_flow(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_filter_equipment:") {
                    self.handle_mgmt_filter_equipment(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_filter_time:") {
                    self.handle_mgmt_filter_time(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_filter_status:") {
                    self.handle_mgmt_filter_status(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_clear_filters:") {
                    self.handle_mgmt_clear_filters(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_page_prev:") {
                    self.handle_mgmt_page_prev(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_page_next:") {
                    self.handle_mgmt_page_next(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_refresh:") {
                    self.handle_mgmt_refresh(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_export:") {
                    self.handle_mgmt_export(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_jump:") {
                    self.handle_mgmt_jump(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_equipment_select:") {
                    self.handle_mgmt_equipment_select(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_time_") {
                    self.handle_mgmt_time_select(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_status_") {
                    self.handle_mgmt_status_select(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("transfer_") {
                    self.handle_equipment_transfer(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("mgmt_transfer_") {
                    self.handle_mgmt_transfer(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("transfer_confirm_") {
                    self.handle_transfer_confirm(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("transfer_cancel_") {
                    self.handle_transfer_cancel(ctx, interaction).await?
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

        // Initialize management state for this user session
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            states.insert(state_key.clone(), ManagementState::default());
        }

        // Show the management dashboard
        self.show_management_dashboard(ctx, interaction, false).await
    }

    async fn show_management_dashboard(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        is_update: bool,
    ) -> Result<()> {
        let guild_id = interaction.guild_id.unwrap().get() as i64;
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        
        let state = {
            let states = MANAGEMENT_STATES.lock().await;
            states.get(&state_key).cloned().unwrap_or_default()
        };

        // Create dashboard embed
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        
        let mut embed = CreateEmbed::new()
            .title("⚙️ Overall Management Dashboard")
            .color(Colour::BLUE);

        // Get filter descriptions for display
        let equipment_desc = if let Some(ref eq_ids) = state.equipment_filter {
            if eq_ids.is_empty() {
                "All Equipment".to_string()
            } else {
                format!("{} selected equipment", eq_ids.len())
            }
        } else {
            "All Equipment".to_string()
        };

        let time_desc = match &state.time_filter {
            TimeFilter::Today => "Today".to_string(),
            TimeFilter::Next24h => "Next 24 hours".to_string(),
            TimeFilter::Next7days => "Next 7 days".to_string(),
            TimeFilter::Custom { start_utc, end_utc } => {
                format!("{} to {}", 
                    crate::time::utc_to_jst_string(*start_utc),
                    crate::time::utc_to_jst_string(*end_utc))
            },
            TimeFilter::All => "All Time".to_string(),
        };

        let status_desc = match state.status_filter {
            StatusFilter::Active => "Active Loans".to_string(),
            StatusFilter::Upcoming => "Upcoming Reservations".to_string(),
            StatusFilter::ReturnedToday => "Returned Today".to_string(),
            StatusFilter::All => "All Statuses".to_string(),
        };

        embed = embed.field("🔧 Current Filters", format!(
            "**Equipment:** {}\n**Time:** {}\n**Status:** {}",
            equipment_desc, time_desc, status_desc
        ), false);

        // Get reservations based on filters
        let reservations = self.get_filtered_reservations(guild_id, &state).await?;
        let total_count = reservations.len();
        let start_idx = state.page * state.items_per_page;
        let end_idx = std::cmp::min(start_idx + state.items_per_page, total_count);
        let page_reservations = &reservations[start_idx..end_idx];

        if page_reservations.is_empty() {
            embed = embed.field("📋 Reservations", "No reservations match the current filters.", false);
        } else {
            let mut reservation_list = String::new();
            for (idx, res) in page_reservations.iter().enumerate() {
                let global_idx = start_idx + idx + 1;
                let equipment_name = self.get_equipment_name(res.equipment_id).await?;
                let status = self.get_reservation_display_status(res).await;
                let start_jst = crate::time::utc_to_jst_string(res.start_time);
                let end_jst = crate::time::utc_to_jst_string(res.end_time);
                let location = res.location.as_deref().unwrap_or("Not specified");

                reservation_list.push_str(&format!(
                    "**{}. {}** {} → {}\n<@{}> • {} • {}\n\n",
                    global_idx, equipment_name, start_jst, end_jst, 
                    res.user_id, status, location
                ));
            }

            embed = embed.field(
                format!("📋 Reservations ({}-{} of {})", start_idx + 1, end_idx, total_count),
                reservation_list,
                false
            );
        }

        // Create quick action buttons for current page reservations (Transfer, Edit, Cancel)
        let mut quick_action_rows = Vec::new();
        if !page_reservations.is_empty() {
            // Create Transfer buttons for displayed reservations (up to 5 per row)
            let mut current_row_buttons = Vec::new();
            for (idx, res) in page_reservations.iter().enumerate() {
                let global_idx = start_idx + idx + 1;
                
                // Only add Transfer button for non-returned reservations
                if res.returned_at.is_none() && res.end_time > chrono::Utc::now() {
                    current_row_buttons.push(
                        CreateButton::new(format!("mgmt_transfer_{}", res.id))
                            .label(format!("🔄 Transfer #{}", global_idx))
                            .style(ButtonStyle::Secondary)
                    );
                    
                    // Discord allows max 5 buttons per row
                    if current_row_buttons.len() >= 5 {
                        quick_action_rows.push(CreateActionRow::Buttons(current_row_buttons));
                        current_row_buttons = Vec::new();
                    }
                }
            }
            
            // Add remaining buttons if any
            if !current_row_buttons.is_empty() {
                quick_action_rows.push(CreateActionRow::Buttons(current_row_buttons));
            }
        }

        // Create filter controls
        let filter_row = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("mgmt_filter_equipment:{}", interaction.token))
                .label("🔧 Equipment Filter")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_filter_time:{}", interaction.token))
                .label("📅 Time Filter")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_filter_status:{}", interaction.token))
                .label("📊 Status Filter")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_clear_filters:{}", interaction.token))
                .label("🗑️ Clear All")
                .style(ButtonStyle::Danger),
        ]);

        // Create pagination controls
        let mut pagination_buttons = vec![];
        if state.page > 0 {
            pagination_buttons.push(
                CreateButton::new(format!("mgmt_page_prev:{}", interaction.token))
                    .label("⬅️ Previous")
                    .style(ButtonStyle::Secondary)
            );
        }
        if end_idx < total_count {
            pagination_buttons.push(
                CreateButton::new(format!("mgmt_page_next:{}", interaction.token))
                    .label("➡️ Next")
                    .style(ButtonStyle::Secondary)
            );
        }

        let pagination_row = if !pagination_buttons.is_empty() {
            Some(CreateActionRow::Buttons(pagination_buttons))
        } else {
            None
        };

        // Create action buttons
        let action_row = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("mgmt_refresh:{}", interaction.token))
                .label("🔄 Refresh Display")
                .style(ButtonStyle::Primary),
            CreateButton::new(format!("mgmt_export:{}", interaction.token))
                .label("📊 Export CSV")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_jump:{}", interaction.token))
                .label("🔗 Jump to Equipment")
                .style(ButtonStyle::Secondary),
        ]);

        let mut components = vec![filter_row];
        
        // Add quick action rows (Transfer buttons) first
        components.extend(quick_action_rows);
        
        // Then add pagination if exists
        if let Some(pagination) = pagination_row {
            components.push(pagination);
        }
        
        // Finally add main action row
        components.push(action_row);

        if is_update {
            let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(components),
            );
            interaction.create_response(&ctx.http, response).await?;
        } else {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(components)
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
        }

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
                } else if interaction.data.custom_id.starts_with("return_modal:") {
                    self.handle_return_modal(ctx, interaction).await?
                } else if interaction.data.custom_id.starts_with("transfer_modal_") {
                    self.handle_transfer_modal_submit(ctx, interaction).await?
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

        let user_id = interaction.user.id.get() as i64;
        
        // Find reservations that can be returned for this user and equipment
        // A reservation can be returned if:
        // 1. It's confirmed and not already returned (returned_at IS NULL)
        // 2. It's currently active (current time is in [start_time, end_time])
        //    OR it's past end_time but still not returned (overdue returns)
        let returnable_reservations = sqlx::query!(
            "SELECT r.id, r.start_time, r.end_time, r.location, e.name as equipment_name
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.equipment_id = ? AND r.user_id = ? AND r.status = 'Confirmed' 
             AND r.returned_at IS NULL 
             AND CURRENT_TIMESTAMP >= r.start_time
             ORDER BY r.start_time ASC",
            equipment_id,
            user_id
        )
        .fetch_all(&self.db)
        .await?;

        if returnable_reservations.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You don't have any active reservations that can be returned for this equipment.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        if returnable_reservations.len() == 1 {
            // Single reservation - proceed directly to return modal
            let reservation = &returnable_reservations[0];
            let reservation_id = reservation.id.unwrap_or(0);
            self.show_return_modal(ctx, interaction, reservation_id, &reservation.equipment_name).await?;
        } else {
            // Multiple reservations - show selection menu
            use serenity::all::{CreateEmbed, CreateActionRow, CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, Colour};
            use crate::time;
            
            let equipment_name = &returnable_reservations[0].equipment_name;
            
            let embed = CreateEmbed::new()
                .title("↩️ Select Reservation to Return")
                .description(format!("**Equipment:** {}\n\nYou have multiple active reservations for this equipment. Please select which one you want to return:", equipment_name))
                .color(Colour::ORANGE);

            let mut options = Vec::new();
            for reservation in &returnable_reservations {
                let reservation_id = reservation.id.unwrap_or(0);
                let start_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
                let end_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));
                let location = reservation.location.as_deref().unwrap_or("Not specified");
                
                options.push(
                    CreateSelectMenuOption::new(
                        format!("{} - {}", start_jst, end_jst),
                        format!("return_reservation_{}", reservation_id)
                    )
                    .description(format!("Location: {}", location))
                );
            }

            let select_menu = CreateSelectMenu::new(
                format!("return_select:{}", interaction.token),
                CreateSelectMenuKind::String { options }
            )
            .placeholder("Choose a reservation to return...")
            .max_values(1);

            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .embed(embed)
                    .components(vec![CreateActionRow::SelectMenu(select_menu)])
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
        }
        
        Ok(())
    }

    // Return flow helper methods
    
    async fn show_return_modal(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        reservation_id: i64,
        equipment_name: &str,
    ) -> Result<()> {
        use serenity::all::{CreateModal, CreateInputText, InputTextStyle};
        
        // Get equipment's default return location
        let equipment = sqlx::query!(
            "SELECT default_return_location FROM equipment 
             WHERE id = (SELECT equipment_id FROM reservations WHERE id = ?)",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;
        
        let default_location = equipment
            .and_then(|e| e.default_return_location)
            .unwrap_or_else(|| "Club Room".to_string());
        
        let modal = CreateModal::new(
            format!("return_modal:{}", reservation_id), 
            format!("Return {}", equipment_name)
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(InputTextStyle::Short, "return_location", "Return Location")
                    .placeholder("Where are you returning this equipment?")
                    .value(default_location)
                    .required(true)
                    .max_length(100),
            ),
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
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

        // Check for conflicts with existing reservations
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

        // Check for conflicts with maintenance windows
        let maintenance_conflicts = sqlx::query!(
            "SELECT id, start_utc, end_utc, reason FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND start_utc < ? AND end_utc > ?",
            equipment_id,
            end_time,
            start_time
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        if !maintenance_conflicts.is_empty() {
            let maintenance = &maintenance_conflicts[0];
            let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(maintenance.start_utc));
            let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(maintenance.end_utc));
            let reason_text = maintenance.reason.as_deref().unwrap_or("Equipment maintenance");
            return Err(format!(
                "Reservation conflicts with scheduled maintenance ({}) from {} to {}. Please choose a different time.",
                reason_text, start_jst, end_jst
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

        // Check for conflicts with maintenance windows
        let maintenance_conflicts = sqlx::query!(
            "SELECT id, start_utc, end_utc, reason FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND start_utc < ? AND end_utc > ?",
            current.equipment_id,
            end_time,
            start_time
        )
        .fetch_all(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        if !maintenance_conflicts.is_empty() {
            let maintenance = &maintenance_conflicts[0];
            let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(maintenance.start_utc));
            let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(maintenance.end_utc));
            let reason_text = maintenance.reason.as_deref().unwrap_or("Equipment maintenance");
            return Err(format!(
                "Updated reservation would conflict with scheduled maintenance ({}) from {} to {}. Please choose a different time.",
                reason_text, start_jst, end_jst
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

    async fn handle_return_modal(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
    ) -> Result<()> {
        // Extract reservation ID from custom_id: "return_modal:{reservation_id}"
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("return_modal:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in return modal: {}", interaction.data.custom_id);
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Error processing return request.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Extract return location from modal
        let mut return_location = String::new();
        
        for row in &interaction.data.components {
            for component in &row.components {
                if let serenity::all::ActionRowComponent::InputText(input_text) = component {
                    if input_text.custom_id == "return_location" {
                        return_location = input_text.value.clone().unwrap_or_default();
                        break;
                    }
                }
            }
        }

        if return_location.trim().is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Return location is required.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Show confirmation screen
        self.show_return_confirmation(ctx, interaction, reservation_id, &return_location).await?;
        Ok(())
    }

    /// Handle modal submission for transfer
    async fn handle_transfer_modal_submit(
        &self,
        ctx: &Context,
        modal: &serenity::all::ModalInteraction,
    ) -> Result<()> {
        let reservation_id_str = modal.data.custom_id
            .strip_prefix("transfer_modal_")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in transfer modal: {}", modal.data.custom_id);
            return Ok(());
        }

        // Extract form data
        let mut new_owner_id_str = String::new();
        let mut transfer_type = String::new();
        let mut schedule_time = String::new();
        let mut note = String::new();

        for action_row in &modal.data.components {
            if let serenity::all::ActionRowComponent::InputText(input) = &action_row.components[0] {
                match input.custom_id.as_str() {
                    "new_owner_id" => new_owner_id_str = input.value.clone().unwrap_or_default(),
                    "transfer_type" => transfer_type = input.value.clone().unwrap_or_default(),
                    "schedule_time" => schedule_time = input.value.clone().unwrap_or_default(),
                    "transfer_note" => note = input.value.clone().unwrap_or_default(),
                    _ => {}
                }
            }
        }

        // Validate inputs
        let new_owner_id: i64 = match new_owner_id_str.parse() {
            Ok(id) => id,
            Err(_) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Invalid User ID format. Please enter a valid Discord User ID.")
                        .ephemeral(true),
                );
                modal.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Validate transfer type
        if transfer_type != "immediate" && transfer_type != "schedule" {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Transfer type must be either 'immediate' or 'schedule'.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Validate new owner exists in guild and is not a bot
        let guild_id = modal.guild_id.unwrap();
        let new_owner_user_id = serenity::all::UserId::new(new_owner_id as u64);
        
        let member = match guild_id.member(&ctx.http, new_owner_user_id).await {
            Ok(member) => member,
            Err(_) => {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ User not found in this server.")
                        .ephemeral(true),
                );
                modal.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        if member.user.bot {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Cannot transfer to a bot user.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Get reservation details and validate
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, r.status, r.returned_at,
                    e.name as equipment_name
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
                        .content("❌ Reservation not found or has been cancelled.")
                        .ephemeral(true),
                );
                modal.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Check if already returned or in the past
        if reservation.returned_at.is_some() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Cannot transfer a returned reservation.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let now_utc = chrono::Utc::now();
        if reservation.end_time <= now_utc {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Cannot transfer a reservation that has ended.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Check permissions: requester must be owner or admin
        let requesting_user_id = modal.user.id.get() as i64;
        let is_owner = reservation.user_id == requesting_user_id;
        let is_admin = utils::is_admin(ctx, guild_id, modal.user.id).await?;

        if !is_owner && !is_admin {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You can only transfer your own reservations, or be an administrator.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Prevent no-op transfers
        if reservation.user_id == new_owner_id {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Cannot transfer to the same user.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Process based on transfer type
        if transfer_type == "immediate" {
            self.execute_immediate_transfer(
                ctx, 
                modal, 
                reservation_id, 
                reservation.user_id, 
                new_owner_id, 
                requesting_user_id,
                if note.is_empty() { None } else { Some(note) }
            ).await?;
        } else {
            // Scheduled transfer
            if schedule_time.is_empty() {
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Schedule time is required for scheduled transfers.")
                        .ephemeral(true),
                );
                modal.create_response(&ctx.http, response).await?;
                return Ok(());
            }

            let execute_at_utc = match crate::time::parse_jst_string(&schedule_time) {
                Some(time) => time,
                None => {
                    let response = serenity::all::CreateInteractionResponse::Message(
                        serenity::all::CreateInteractionResponseMessage::new()
                            .content("❌ Invalid time format. Please use YYYY-MM-DD HH:MM in JST.")
                            .ephemeral(true),
                    );
                    modal.create_response(&ctx.http, response).await?;
                    return Ok(());
                }
            };

            // Validate scheduled time is within reservation window
            let min_execute_time = std::cmp::max(now_utc, reservation.start_time);
            if execute_at_utc <= now_utc || execute_at_utc < min_execute_time || execute_at_utc >= reservation.end_time {
                let min_jst = crate::time::utc_to_jst_string(min_execute_time);
                let max_jst = crate::time::utc_to_jst_string(reservation.end_time);
                let response = serenity::all::CreateInteractionResponse::Message(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!(
                            "❌ Transfer time must be between {} and {} (JST).",
                            min_jst, max_jst
                        ))
                        .ephemeral(true),
                );
                modal.create_response(&ctx.http, response).await?;
                return Ok(());
            }

            self.create_scheduled_transfer(
                ctx,
                modal,
                reservation_id,
                reservation.user_id,
                new_owner_id,
                requesting_user_id,
                execute_at_utc,
                if note.is_empty() { None } else { Some(note) }
            ).await?;
        }

        Ok(())
    }

    async fn handle_return_select(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Extract selected reservation ID from select menu data
        let selected_value = if let serenity::all::ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
            values.first().cloned().unwrap_or_default()
        } else {
            String::new()
        };

        let reservation_id_str = selected_value
            .strip_prefix("return_reservation_")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in return select: {}", selected_value);
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Error processing return request.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Get equipment name for the modal
        let equipment = sqlx::query!(
            "SELECT e.name FROM equipment e 
             JOIN reservations r ON e.id = r.equipment_id 
             WHERE r.id = ?",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let equipment_name = equipment
            .map(|e| e.name)
            .unwrap_or_else(|| "Unknown Equipment".to_string());

        self.show_return_modal(ctx, interaction, reservation_id, &equipment_name).await?;
        Ok(())
    }

    async fn show_return_confirmation(
        &self,
        ctx: &Context,
        interaction: &ModalInteraction,
        reservation_id: i64,
        return_location: &str,
    ) -> Result<()> {
        use serenity::all::{CreateEmbed, CreateActionRow, CreateButton, ButtonStyle, Colour};
        use crate::time;
        
        // Get reservation details for confirmation
        let reservation = sqlx::query!(
            "SELECT r.start_time, r.end_time, r.location, e.name as equipment_name
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
                        .content("❌ Reservation not found or has been cancelled.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        let start_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
        let end_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));
        let original_location = reservation.location.as_deref().unwrap_or("Not specified");

        let embed = CreateEmbed::new()
            .title("↩️ Confirm Equipment Return")
            .description(format!(
                "**Equipment:** {}\n**Reservation Period:** {} to {}\n**Original Location:** {}\n**Return Location:** {}\n\nPlease confirm that you want to return this equipment now.",
                reservation.equipment_name,
                start_jst,
                end_jst,
                original_location,
                return_location
            ))
            .color(Colour::ORANGE)
            .footer(serenity::all::CreateEmbedFooter::new("This action cannot be undone without admin assistance"));

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("confirm_return:{}", reservation_id))
                .label("✅ Confirm Return")
                .style(ButtonStyle::Success),
            CreateButton::new(format!("cancel_return:{}", reservation_id))
                .label("❌ Cancel")
                .style(ButtonStyle::Secondary),
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

    async fn handle_confirm_return(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // Extract reservation ID from custom_id: "confirm_return:{reservation_id}"
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("confirm_return:")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in confirm return: {}", interaction.data.custom_id);
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Error processing return request.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        let user_id = interaction.user.id.get() as i64;

        // Extract return location from the original embed description
        let return_location = if let Some(embed) = interaction.message.embeds.first() {
            if let Some(description) = &embed.description {
                // Parse return location from description
                if let Some(start) = description.find("**Return Location:** ") {
                    let location_start = start + "**Return Location:** ".len();
                    if let Some(end) = description[location_start..].find('\n') {
                        description[location_start..location_start + end].to_string()
                    } else {
                        description[location_start..].to_string()
                    }
                } else {
                    "Club Room".to_string()
                }
            } else {
                "Club Room".to_string()
            }
        } else {
            "Club Room".to_string()
        };

        // Process the return in a transaction
        match self.process_equipment_return(reservation_id, user_id, &return_location).await {
            Ok((equipment_name, reservation_details)) => {
                use crate::time;
                let return_time_jst = time::utc_to_jst_string(chrono::Utc::now());
                
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!(
                            "✅ **Equipment Returned Successfully!**\n\n📦 **Equipment:** {}\n📍 **Return Location:** {}\n🕐 **Return Time:** {}\n\n{}",
                            equipment_name,
                            return_location,
                            return_time_jst,
                            reservation_details
                        ))
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;

                // Trigger reconcile for the equipment channel
                if let Some(guild_id) = interaction.guild_id {
                    if let Err(e) = self.reconcile_equipment_displays(ctx, guild_id.get() as i64).await {
                        error!("Failed to reconcile equipment displays after return: {}", e);
                    }
                }
            }
            Err(err_msg) => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content(format!("❌ **Failed to Return Equipment**\n\n{}", err_msg))
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
            }
        }

        Ok(())
    }

    async fn handle_cancel_return_flow(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("❌ Return cancelled.")
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn process_equipment_return(
        &self,
        reservation_id: i64,
        user_id: i64,
        return_location: &str,
    ) -> Result<(String, String), String> {
        // Start transaction
        let mut tx = self.db.begin().await.map_err(|e| format!("Database error: {}", e))?;

        // Get reservation and equipment details
        let reservation = sqlx::query!(
            "SELECT r.equipment_id, r.user_id, r.start_time, r.end_time, r.location, r.returned_at,
                    e.name as equipment_name, e.status as equipment_status
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| format!("Database error: {}", e))?
        .ok_or("Reservation not found")?;

        // Verify ownership
        if reservation.user_id != user_id {
            return Err("You can only return your own reservations".to_string());
        }

        // Check if already returned
        if reservation.returned_at.is_some() {
            return Err("This reservation has already been returned".to_string());
        }

        // Check if reservation is active or past (can return overdue items)
        let now = chrono::Utc::now().naive_utc();
        if reservation.start_time > now {
            return Err("Cannot return equipment before the reservation period starts".to_string());
        }

        let return_time = chrono::Utc::now();
        let return_time_naive = return_time.naive_utc();

        // Update reservation with return information
        sqlx::query!(
            "UPDATE reservations 
             SET returned_at = ?, return_location = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
            return_time_naive,
            return_location,
            reservation_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update reservation: {}", e))?;

        // Update equipment status to Available and set current location
        sqlx::query!(
            "UPDATE equipment 
             SET status = 'Available', current_location = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
            return_location,
            reservation.equipment_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to update equipment: {}", e))?;

        // Log the return event
        let log_notes = format!("Returned from reservation {}", reservation_id);
        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'Returned', ?, ?, 'Available', ?, ?)",
            reservation.equipment_id,
            user_id,
            return_location,
            reservation.equipment_status,
            log_notes,
            return_time_naive
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| format!("Failed to log return: {}", e))?;

        tx.commit().await.map_err(|e| format!("Failed to commit transaction: {}", e))?;

        use crate::time;
        let start_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.start_time));
        let end_jst = time::utc_to_jst_string(Self::naive_datetime_to_utc(reservation.end_time));
        let original_location = reservation.location.as_deref().unwrap_or("Not specified");

        let details = format!(
            "📅 **Reservation Period:** {} to {}\n📍 **Original Location:** {}",
            start_jst, end_jst, original_location
        );

        Ok((reservation.equipment_name, details))
    }

    async fn reconcile_equipment_displays(&self, ctx: &Context, guild_id: i64) -> Result<()> {
        // Get reservation channel for this guild
        let guild = sqlx::query!(
            "SELECT reservation_channel_id FROM guilds WHERE id = ?",
            guild_id
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(guild_data) = guild {
            if let Some(channel_id) = guild_data.reservation_channel_id {
                // Trigger equipment rendering reconcile
                let equipment_renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                if let Err(e) = equipment_renderer.reconcile_equipment_display(ctx, guild_id, channel_id).await {
                    error!("Failed to reconcile equipment channel: {}", e);
                }
            }
        }

        Ok(())
    }

    pub async fn get_filtered_reservations(
        &self,
        guild_id: i64,
        state: &ManagementState,
    ) -> Result<Vec<crate::models::Reservation>> {
        // For now, implement a simple version that gets all reservations
        // In production, you'd want proper dynamic query building with the filters
        let reservations = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, r.location, r.status, r.created_at, r.updated_at, r.returned_at, r.return_location 
             FROM reservations r 
             JOIN equipment e ON r.equipment_id = e.id 
             WHERE e.guild_id = ? AND r.status = 'Confirmed'
             ORDER BY r.start_time ASC",
            guild_id
        )
        .fetch_all(&self.db)
        .await?;

        // Convert to proper Reservation structs
        let reservations: Vec<crate::models::Reservation> = reservations.into_iter().map(|row| {
            use chrono::{DateTime, Utc};
            
            crate::models::Reservation {
                id: row.id.unwrap_or(0),
                equipment_id: row.equipment_id,
                user_id: row.user_id,
                start_time: DateTime::<Utc>::from_naive_utc_and_offset(row.start_time, Utc),
                end_time: DateTime::<Utc>::from_naive_utc_and_offset(row.end_time, Utc),
                location: row.location,
                status: row.status,
                created_at: DateTime::<Utc>::from_naive_utc_and_offset(row.created_at.unwrap_or_default(), Utc),
                updated_at: DateTime::<Utc>::from_naive_utc_and_offset(row.updated_at.unwrap_or_default(), Utc),
                returned_at: row.returned_at.map(|dt| DateTime::<Utc>::from_naive_utc_and_offset(dt, Utc)),
                return_location: row.return_location,
            }
        }).collect();

        // Apply filters in memory for now (not optimal for large datasets)
        let filtered: Vec<_> = reservations.into_iter()
            .filter(|r| self.matches_equipment_filter(r, &state.equipment_filter))
            .filter(|r| self.matches_time_filter(r, &state.time_filter))
            .filter(|r| self.matches_status_filter(r, &state.status_filter))
            .collect();

        Ok(filtered)
    }

    pub fn matches_equipment_filter(&self, reservation: &crate::models::Reservation, filter: &Option<Vec<i64>>) -> bool {
        match filter {
            None => true, // No filter means all equipment
            Some(ids) => ids.is_empty() || ids.contains(&reservation.equipment_id),
        }
    }

    pub fn matches_time_filter(&self, reservation: &crate::models::Reservation, filter: &TimeFilter) -> bool {
        match filter {
            TimeFilter::All => true,
            TimeFilter::Today => {
                let today_start = chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
                let today_end = today_start + chrono::Duration::days(1);
                let today_start_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_start, Utc);
                let today_end_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_end, Utc);
                // Reservation overlaps with today
                reservation.start_time <= today_end_utc && reservation.end_time >= today_start_utc
            },
            TimeFilter::Next24h => {
                let now = chrono::Utc::now();
                let next_24h = now + chrono::Duration::hours(24);
                reservation.start_time <= next_24h && reservation.start_time >= now
            },
            TimeFilter::Next7days => {
                let now = chrono::Utc::now();
                let next_7days = now + chrono::Duration::days(7);
                reservation.start_time <= next_7days && reservation.start_time >= now
            },
            TimeFilter::Custom { start_utc, end_utc } => {
                reservation.start_time <= *end_utc && reservation.end_time >= *start_utc
            },
        }
    }

    pub fn matches_status_filter(&self, reservation: &crate::models::Reservation, filter: &StatusFilter) -> bool {
        let now = chrono::Utc::now();
        
        match filter {
            StatusFilter::All => true,
            StatusFilter::Active => {
                // Currently active and not returned
                reservation.returned_at.is_none() 
                    && now >= reservation.start_time 
                    && now <= reservation.end_time
            },
            StatusFilter::Upcoming => {
                // Future reservations
                now < reservation.start_time
            },
            StatusFilter::ReturnedToday => {
                // Returned today
                if let Some(returned_at) = reservation.returned_at {
                    let today_start = chrono::Utc::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
                    let today_end = today_start + chrono::Duration::days(1);
                    let today_start_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_start, Utc);
                    let today_end_utc = DateTime::<Utc>::from_naive_utc_and_offset(today_end, Utc);
                    returned_at >= today_start_utc && returned_at < today_end_utc
                } else {
                    false
                }
            },
        }
    }

    pub async fn get_equipment_name(&self, equipment_id: i64) -> Result<String> {
        let name: Option<String> = sqlx::query_scalar(
            "SELECT name FROM equipment WHERE id = ?"
        )
        .bind(equipment_id)
        .fetch_optional(&self.db)
        .await?;

        Ok(name.unwrap_or_else(|| format!("Equipment #{}", equipment_id)))
    }

    pub async fn get_reservation_display_status(&self, reservation: &crate::models::Reservation) -> String {
        let now = chrono::Utc::now();
        
        if let Some(_) = reservation.returned_at {
            "Returned".to_string()
        } else if now >= reservation.start_time && now <= reservation.end_time {
            "Active".to_string()
        } else if now < reservation.start_time {
            "Upcoming".to_string()
        } else {
            "Overdue".to_string()
        }
    }

    async fn handle_mgmt_filter_equipment(
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

        // Get equipment for selection
        let guild_id = interaction.guild_id.unwrap().get() as i64;
        let equipment = sqlx::query!(
            "SELECT id, name FROM equipment WHERE guild_id = ? ORDER BY name",
            guild_id
        )
        .fetch_all(&self.db)
        .await?;

        if equipment.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ No equipment found. Add equipment first using the Overall Management panel.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        use serenity::all::{CreateSelectMenu, CreateSelectMenuKind, CreateSelectMenuOption, CreateActionRow};

        let mut options = vec![
            CreateSelectMenuOption::new("All Equipment", "all")
                .description("Show reservations for all equipment")
        ];

        for eq in equipment.iter().take(24) { // Discord limit of 25 options
            let eq_id = eq.id.unwrap_or(0); // Handle potential NULL ids
            options.push(
                CreateSelectMenuOption::new(&eq.name, eq_id.to_string())
                    .description(format!("Filter by {}", eq.name))
            );
        }

        let options_len = options.len();
        let select = CreateSelectMenu::new(
            format!("mgmt_equipment_select:{}", interaction.token),
            CreateSelectMenuKind::String { options }
        )
        .placeholder("Select equipment to filter by...")
        .min_values(1)
        .max_values(std::cmp::min(options_len as u8, 25));

        let components = vec![CreateActionRow::SelectMenu(select)];

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("🔧 **Equipment Filter**\nSelect which equipment to show reservations for:")
                .components(components),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_mgmt_filter_time(
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

        use serenity::all::{CreateButton, CreateActionRow, ButtonStyle};

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("mgmt_time_today:{}", interaction.token))
                .label("📅 Today")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_time_24h:{}", interaction.token))
                .label("🕐 Next 24h")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_time_7d:{}", interaction.token))
                .label("📊 Next 7 days")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_time_custom:{}", interaction.token))
                .label("⚙️ Custom")
                .style(ButtonStyle::Primary),
            CreateButton::new(format!("mgmt_time_all:{}", interaction.token))
                .label("🌐 All Time")
                .style(ButtonStyle::Danger),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("📅 **Time Filter**\nSelect time range for reservations:")
                .components(vec![buttons]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_mgmt_filter_status(
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

        use serenity::all::{CreateButton, CreateActionRow, ButtonStyle};

        let buttons = CreateActionRow::Buttons(vec![
            CreateButton::new(format!("mgmt_status_active:{}", interaction.token))
                .label("🟢 Active")
                .style(ButtonStyle::Success),
            CreateButton::new(format!("mgmt_status_upcoming:{}", interaction.token))
                .label("🟡 Upcoming")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_status_returned:{}", interaction.token))
                .label("🔄 Returned Today")
                .style(ButtonStyle::Secondary),
            CreateButton::new(format!("mgmt_status_all:{}", interaction.token))
                .label("📊 All Status")
                .style(ButtonStyle::Primary),
        ]);

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("📊 **Status Filter**\nSelect reservation status:")
                .components(vec![buttons]),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    async fn handle_mgmt_clear_filters(
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

        // Reset filters to default
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            states.insert(state_key, ManagementState::default());
        }

        // Update dashboard
        self.show_management_dashboard(ctx, interaction, true).await
    }

    async fn handle_mgmt_page_prev(
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

        // Update page in state
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                if state.page > 0 {
                    state.page -= 1;
                }
            }
        }

        // Update dashboard
        self.show_management_dashboard(ctx, interaction, true).await
    }

    async fn handle_mgmt_page_next(
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

        // Update page in state
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.page += 1;
            }
        }

        // Update dashboard
        self.show_management_dashboard(ctx, interaction, true).await
    }

    async fn handle_mgmt_refresh(
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

        // Trigger reconcile for the reservation channel
        let guild_id = interaction.guild_id.unwrap().get() as i64;

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("🔄 Refreshing display... Please wait.")
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;

        // Perform the refresh
        let result = self.reconcile_guild_display(ctx, guild_id).await;

        let followup_content = match result {
            Ok(_) => "✅ Display refreshed successfully!",
            Err(e) => {
                error!("Failed to refresh display: {}", e);
                "❌ Failed to refresh display. Check logs for details."
            }
        };

        // Send follow-up message
        interaction.edit_response(&ctx.http, 
            serenity::all::EditInteractionResponse::new()
                .content(followup_content)
        ).await?;

        Ok(())
    }

    async fn handle_mgmt_export(
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

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("📊 Generating CSV export... Please wait.")
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;

        // Get filtered reservations for export
        let guild_id = interaction.guild_id.unwrap().get() as i64;
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        
        let state = {
            let states = MANAGEMENT_STATES.lock().await;
            states.get(&state_key).cloned().unwrap_or_default()
        };

        let reservations = self.get_filtered_reservations(guild_id, &state).await?;
        let reservation_count = reservations.len();
        
        // Generate CSV content
        let mut csv_content = String::new();
        csv_content.push_str("Reservation ID,Equipment,User ID,Start Time (JST),End Time (JST),Start Time (UTC),End Time (UTC),Status,Location,Returned At (JST),Return Location\n");
        
        for res in &reservations {
            let equipment_name = self.get_equipment_name(res.equipment_id).await?;
            let status = self.get_reservation_display_status(&res).await;
            let start_jst = crate::time::utc_to_jst_string(res.start_time);
            let end_jst = crate::time::utc_to_jst_string(res.end_time);
            let location = res.location.as_deref().unwrap_or("Not specified");
            let returned_jst = res.returned_at.map(|dt| crate::time::utc_to_jst_string(dt)).unwrap_or_default();
            let return_location = res.return_location.as_deref().unwrap_or("");
            
            csv_content.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{}\n",
                res.id,
                equipment_name.replace(",", ";"), // Escape commas
                res.user_id,
                start_jst,
                end_jst,
                res.start_time.format("%Y-%m-%d %H:%M:%S UTC"),
                res.end_time.format("%Y-%m-%d %H:%M:%S UTC"),
                status,
                location.replace(",", ";"), // Escape commas
                returned_jst,
                return_location.replace(",", ";") // Escape commas
            ));
        }

        // For now, show a summary instead of actual file download
        let summary = format!(
            "📊 **CSV Export Summary**\n\
            **Total Reservations:** {}\n\
            **Applied Filters:**\n\
            • Equipment: {}\n\
            • Time: {}\n\
            • Status: {}\n\n\
            *CSV download feature coming soon. Data preview:*\n\
            ```\n{}```",
            reservation_count,
            if state.equipment_filter.is_some() && !state.equipment_filter.as_ref().unwrap().is_empty() {
                format!("{} selected", state.equipment_filter.as_ref().unwrap().len())
            } else {
                "All".to_string()
            },
            match state.time_filter {
                TimeFilter::Today => "Today",
                TimeFilter::Next24h => "Next 24h",
                TimeFilter::Next7days => "Next 7 days",
                TimeFilter::Custom { .. } => "Custom",
                TimeFilter::All => "All Time",
            },
            match state.status_filter {
                StatusFilter::Active => "Active",
                StatusFilter::Upcoming => "Upcoming",
                StatusFilter::ReturnedToday => "Returned Today",
                StatusFilter::All => "All",
            },
            csv_content.lines().take(6).collect::<Vec<_>>().join("\n")
        );

        // Send follow-up message
        interaction.edit_response(&ctx.http, 
            serenity::all::EditInteractionResponse::new()
                .content(summary)
        ).await?;

        Ok(())
    }

    async fn handle_mgmt_jump(
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

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("🔗 Jump to equipment feature coming soon. For now, navigate manually to the reservation channel.")
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;

        Ok(())
    }

    async fn handle_mgmt_equipment_select(
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

        use serenity::all::ComponentInteractionDataKind;
        
        // Extract selected equipment IDs
        let selected_equipment = if let ComponentInteractionDataKind::StringSelect { values } = &interaction.data.kind {
            if values.contains(&"all".to_string()) {
                None // All equipment
            } else {
                Some(values.iter().filter_map(|v| v.parse::<i64>().ok()).collect::<Vec<_>>())
            }
        } else {
            return Ok(());
        };

        // Update filter state
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.equipment_filter = selected_equipment;
                state.page = 0; // Reset to first page
            }
        }

        // Update dashboard
        self.show_management_dashboard(ctx, interaction, true).await
    }

    async fn handle_mgmt_time_select(
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

        // Determine which time filter was selected
        let time_filter = if interaction.data.custom_id.contains("mgmt_time_today:") {
            TimeFilter::Today
        } else if interaction.data.custom_id.contains("mgmt_time_24h:") {
            TimeFilter::Next24h
        } else if interaction.data.custom_id.contains("mgmt_time_7d:") {
            TimeFilter::Next7days
        } else if interaction.data.custom_id.contains("mgmt_time_custom:") {
            // For now, just use All as custom implementation would need a modal
            TimeFilter::All
        } else if interaction.data.custom_id.contains("mgmt_time_all:") {
            TimeFilter::All
        } else {
            TimeFilter::All
        };

        // Update filter state
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.time_filter = time_filter;
                state.page = 0; // Reset to first page
            }
        }

        // Update dashboard
        self.show_management_dashboard(ctx, interaction, true).await
    }

    async fn handle_mgmt_status_select(
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

        // Determine which status filter was selected
        let status_filter = if interaction.data.custom_id.contains("mgmt_status_active:") {
            StatusFilter::Active
        } else if interaction.data.custom_id.contains("mgmt_status_upcoming:") {
            StatusFilter::Upcoming
        } else if interaction.data.custom_id.contains("mgmt_status_returned:") {
            StatusFilter::ReturnedToday
        } else {
            StatusFilter::All
        };

        // Update filter state
        let state_key = (interaction.guild_id.unwrap(), interaction.user.id, interaction.token.clone());
        {
            let mut states = MANAGEMENT_STATES.lock().await;
            if let Some(state) = states.get_mut(&state_key) {
                state.status_filter = status_filter;
                state.page = 0; // Reset to first page
            }
        }

        // Update dashboard
        self.show_management_dashboard(ctx, interaction, true).await
    }

    async fn reconcile_guild_display(&self, ctx: &Context, guild_id: i64) -> Result<()> {
        let guild = sqlx::query!(
            "SELECT reservation_channel_id FROM guilds WHERE id = ?",
            guild_id
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(guild_data) = guild {
            if let Some(channel_id) = guild_data.reservation_channel_id {
                // Trigger equipment rendering reconcile
                let equipment_renderer = crate::equipment::EquipmentRenderer::new(self.db.clone());
                equipment_renderer.reconcile_equipment_display(ctx, guild_id, channel_id).await?;
            }
        }

        Ok(())
    }

    // ==================== TRANSFER HANDLERS ====================

    /// Handle transfer button from equipment embed
    async fn handle_equipment_transfer(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let equipment_id_str = interaction.data.custom_id
            .strip_prefix("transfer_")
            .unwrap_or("");
            
        let equipment_id: i64 = equipment_id_str.parse().unwrap_or(0);
        if equipment_id == 0 {
            error!("Invalid equipment ID in transfer button: {}", interaction.data.custom_id);
            return Ok(());
        }

        let user_id = interaction.user.id.get() as i64;
        let guild_id = interaction.guild_id.unwrap();

        // Check if user has any active or upcoming reservations for this equipment
        let user_reservations = sqlx::query!(
            "SELECT id, start_time, end_time, status, user_id FROM reservations 
             WHERE equipment_id = ? AND user_id = ? AND status = 'Confirmed' 
             AND end_time > CURRENT_TIMESTAMP AND returned_at IS NULL
             ORDER BY start_time ASC",
            equipment_id,
            user_id
        )
        .fetch_all(&self.db)
        .await?;

        // Also check if user is admin - admins can transfer any reservation
        let is_admin = utils::is_admin(ctx, guild_id, interaction.user.id).await?;

        if user_reservations.is_empty() && !is_admin {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You don't have any active or upcoming reservations for this equipment.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // If admin and no personal reservations, find any active/upcoming reservations
        let available_reservations = if user_reservations.is_empty() && is_admin {
            sqlx::query!(
                "SELECT id, start_time, end_time, status, user_id FROM reservations 
                 WHERE equipment_id = ? AND status = 'Confirmed' 
                 AND end_time > CURRENT_TIMESTAMP AND returned_at IS NULL
                 ORDER BY start_time ASC",
                equipment_id
            )
            .fetch_all(&self.db)
            .await?
        } else {
            user_reservations
        };

        if available_reservations.is_empty() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ No active or upcoming reservations found for this equipment.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Show transfer modal for the first available reservation
        self.show_transfer_modal(ctx, interaction, available_reservations[0].id).await
    }

    /// Handle transfer action from Overall Management panel
    async fn handle_mgmt_transfer(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let reservation_id_str = interaction.data.custom_id
            .strip_prefix("mgmt_transfer_")
            .unwrap_or("");
            
        let reservation_id: i64 = reservation_id_str.parse().unwrap_or(0);
        if reservation_id == 0 {
            error!("Invalid reservation ID in mgmt transfer button: {}", interaction.data.custom_id);
            return Ok(());
        }

        let user_id = interaction.user.id.get() as i64;
        let guild_id = interaction.guild_id.unwrap();

        // Get reservation details
        let reservation = sqlx::query!(
            "SELECT id, equipment_id, user_id, start_time, end_time, status, returned_at
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
                        .content("❌ Reservation not found or has been cancelled.")
                        .ephemeral(true),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Check if already returned
        if reservation.returned_at.is_some() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ Cannot transfer a returned reservation.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Check permissions: owner or admin
        let is_owner = reservation.user_id == user_id;
        let is_admin = utils::is_admin(ctx, guild_id, interaction.user.id).await?;

        if !is_owner && !is_admin {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You can only transfer your own reservations, or be an administrator.")
                    .ephemeral(true),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Show transfer modal
        self.show_transfer_modal(ctx, interaction, reservation_id).await
    }

    /// Show transfer modal for user selection
    async fn show_transfer_modal(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
        reservation_id: i64,
    ) -> Result<()> {
        use serenity::all::{CreateInputText, CreateModal, InputTextStyle};

        // Get reservation details for display
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, e.name as equipment_name
             FROM reservations r
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ?",
            reservation_id
        )
        .fetch_one(&self.db)
        .await?;

        let start_jst = crate::time::utc_to_jst_string(reservation.start_time);
        let end_jst = crate::time::utc_to_jst_string(reservation.end_time);

        let modal = CreateModal::new(
            format!("transfer_modal_{}", reservation_id),
            format!("Transfer Reservation - {}", reservation.equipment_name)
        )
        .components(vec![
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(
                    InputTextStyle::Short,
                    "New Owner User ID",
                    "new_owner_id"
                )
                .placeholder("Enter the Discord User ID of the new owner")
                .required(true)
                .min_length(17) // Discord snowflake min length
                .max_length(20)  // Discord snowflake max length
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(
                    InputTextStyle::Short,
                    "Transfer Type",
                    "transfer_type"
                )
                .placeholder("immediate or schedule")
                .value("immediate")
                .required(true)
                .min_length(9)
                .max_length(9)
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(
                    InputTextStyle::Short,
                    "Schedule Time (JST)",
                    "schedule_time"
                )
                .placeholder("YYYY-MM-DD HH:MM (only if type=schedule)")
                .required(false)
                .min_length(16)
                .max_length(16)
            ),
            serenity::all::CreateActionRow::InputText(
                CreateInputText::new(
                    InputTextStyle::Paragraph,
                    "Note (Optional)",
                    "transfer_note"
                )
                .placeholder("Optional note for the transfer")
                .required(false)
                .max_length(500)
            )
        ]);

        let response = serenity::all::CreateInteractionResponse::Modal(modal);
        interaction.create_response(&ctx.http, response).await?;

        Ok(())
    }

    /// Handle transfer cancellation
    async fn handle_transfer_cancel(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        let transfer_id_str = interaction.data.custom_id
            .strip_prefix("transfer_cancel_")
            .unwrap_or("");
            
        let transfer_id: i64 = transfer_id_str.parse().unwrap_or(0);
        if transfer_id == 0 {
            error!("Invalid transfer ID in cancel button: {}", interaction.data.custom_id);
            return Ok(());
        }

        let user_id = interaction.user.id.get() as i64;
        let guild_id = interaction.guild_id.unwrap();

        // Get transfer request details
        let transfer = sqlx::query!(
            "SELECT id, reservation_id, from_user_id, to_user_id, requested_by_user_id, status, execute_at_utc
             FROM transfer_requests WHERE id = ? AND status = 'Pending'",
            transfer_id
        )
        .fetch_optional(&self.db)
        .await?;

        let transfer = match transfer {
            Some(t) => t,
            None => {
                let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                    serenity::all::CreateInteractionResponseMessage::new()
                        .content("❌ Transfer request not found or already processed.")
                        .components(vec![]),
                );
                interaction.create_response(&ctx.http, response).await?;
                return Ok(());
            }
        };

        // Check permissions: original requester or admin can cancel
        let is_requester = transfer.requested_by_user_id == user_id;
        let is_admin = utils::is_admin(ctx, guild_id, interaction.user.id).await?;

        if !is_requester && !is_admin {
            let response = serenity::all::CreateInteractionResponse::UpdateMessage(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ You can only cancel transfer requests you created, or be an administrator.")
                    .components(vec![]),
            );
            interaction.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Cancel the transfer request
        let now = chrono::Utc::now();
        sqlx::query!(
            "UPDATE transfer_requests 
             SET status = 'Canceled', canceled_at_utc = ?, canceled_by_user_id = ?, updated_at = ?
             WHERE id = ?",
            now,
            user_id,
            now,
            transfer_id
        )
        .execute(&self.db)
        .await?;

        let execute_jst = transfer.execute_at_utc
            .map(|t| crate::time::utc_to_jst_string(t))
            .unwrap_or_else(|| "Unknown".to_string());

        let response = serenity::all::CreateInteractionResponse::UpdateMessage(
            serenity::all::CreateInteractionResponseMessage::new()
                .content(format!(
                    "🚫 **Transfer Cancelled**\n\n👤 **From:** <@{}>\n👤 **To:** <@{}>\n🕐 **Was Scheduled For:** {} (JST)\n✅ **Cancelled by:** <@{}>",
                    transfer.from_user_id,
                    transfer.to_user_id,
                    execute_jst,
                    user_id
                ))
                .components(vec![]),
        );
        interaction.create_response(&ctx.http, response).await?;

        Ok(())
    }

    /// Handle transfer confirmation (for future use with scheduled transfers)
    async fn handle_transfer_confirm(
        &self,
        ctx: &Context,
        interaction: &ComponentInteraction,
    ) -> Result<()> {
        // This would be used for target user confirmation in future iterations
        // For now, scheduled transfers execute automatically
        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .content("⚙️ Transfer confirmation feature coming soon.")
                .ephemeral(true),
        );
        interaction.create_response(&ctx.http, response).await?;
        Ok(())
    }

    /// Execute immediate transfer
    async fn execute_immediate_transfer(
        &self,
        ctx: &Context,
        modal: &serenity::all::ModalInteraction,
        reservation_id: i64,
        from_user_id: i64,
        to_user_id: i64,
        requesting_user_id: i64,
        note: Option<String>,
    ) -> Result<()> {
        let mut tx = self.db.begin().await?;

        // Update reservation owner
        sqlx::query!(
            "UPDATE reservations SET user_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            to_user_id,
            reservation_id
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to update reservation owner: {}", e))?;

        // Get equipment details for logging
        let equipment = sqlx::query!(
            "SELECT e.id, e.name FROM equipment e 
             JOIN reservations r ON e.id = r.equipment_id 
             WHERE r.id = ?",
            reservation_id
        )
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get equipment details: {}", e))?;

        // Log the transfer
        let log_note = format!(
            "Transferred from <@{}> to <@{}> by <@{}> - Reservation ID: {}{}",
            from_user_id,
            to_user_id,
            requesting_user_id,
            reservation_id,
            if let Some(n) = &note { format!(" - Note: {}", n) } else { String::new() }
        );

        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'Transferred', NULL, 'Confirmed', 'Confirmed', ?, CURRENT_TIMESTAMP)",
            equipment.id,
            requesting_user_id,
            log_note
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to log transfer: {}", e))?;

        tx.commit().await
            .map_err(|e| anyhow::anyhow!("Failed to commit transfer transaction: {}", e))?;

        // Send success response
        let reservation_details = sqlx::query!(
            "SELECT r.start_time, r.end_time, e.name as equipment_name
             FROM reservations r
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ?",
            reservation_id
        )
        .fetch_one(&self.db)
        .await?;

        let start_jst = crate::time::utc_to_jst_string(reservation_details.start_time);
        let end_jst = crate::time::utc_to_jst_string(reservation_details.end_time);

        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .content(format!(
                    "✅ **Reservation Transferred Successfully!**\n\n📦 **Equipment:** {}\n👤 **From:** <@{}>\n👤 **To:** <@{}>\n🕐 **Period:** {} - {} (JST){}",
                    reservation_details.equipment_name,
                    from_user_id,
                    to_user_id,
                    start_jst,
                    end_jst,
                    if let Some(n) = note { format!("\n📝 **Note:** {}", n) } else { String::new() }
                ))
                .ephemeral(true),
        );
        modal.create_response(&ctx.http, response).await?;

        // Trigger equipment display reconciliation
        let guild_id = modal.guild_id.unwrap().get() as i64;
        if let Err(e) = self.reconcile_guild_display(ctx, guild_id).await {
            error!("Failed to reconcile display after transfer: {}", e);
        }

        // TODO: Send DM notifications to old and new owners (best-effort)
        // This would use the notification infrastructure from PR #16

        Ok(())
    }

    /// Create scheduled transfer request
    async fn create_scheduled_transfer(
        &self,
        ctx: &Context,
        modal: &serenity::all::ModalInteraction,
        reservation_id: i64,
        from_user_id: i64,
        to_user_id: i64,
        requesting_user_id: i64,
        execute_at_utc: chrono::DateTime<chrono::Utc>,
        note: Option<String>,
    ) -> Result<()> {
        // Check for existing pending transfer for this reservation
        let existing = sqlx::query!(
            "SELECT id FROM transfer_requests 
             WHERE reservation_id = ? AND status = 'Pending'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        if existing.is_some() {
            let response = serenity::all::CreateInteractionResponse::Message(
                serenity::all::CreateInteractionResponseMessage::new()
                    .content("❌ A transfer request for this reservation is already pending.")
                    .ephemeral(true),
            );
            modal.create_response(&ctx.http, response).await?;
            return Ok(());
        }

        // Create transfer request
        let expires_at = execute_at_utc + chrono::Duration::hours(1); // Give 1 hour buffer for execution
        let now = chrono::Utc::now();

        let transfer_id = sqlx::query!(
            "INSERT INTO transfer_requests 
             (reservation_id, from_user_id, to_user_id, requested_by_user_id, execute_at_utc, note, expires_at, status, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 'Pending', ?, ?) 
             RETURNING id",
            reservation_id,
            from_user_id,
            to_user_id,
            requesting_user_id,
            execute_at_utc,
            note,
            expires_at,
            now,
            now
        )
        .fetch_one(&self.db)
        .await?
        .id;

        // Get reservation details for response
        let reservation_details = sqlx::query!(
            "SELECT r.start_time, r.end_time, e.name as equipment_name
             FROM reservations r
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ?",
            reservation_id
        )
        .fetch_one(&self.db)
        .await?;

        let execute_jst = crate::time::utc_to_jst_string(execute_at_utc);

        let response = serenity::all::CreateInteractionResponse::Message(
            serenity::all::CreateInteractionResponseMessage::new()
                .content(format!(
                    "✅ **Transfer Scheduled!**\n\n📦 **Equipment:** {}\n👤 **From:** <@{}>\n👤 **To:** <@{}>\n🕐 **Execute At:** {} (JST){}",
                    reservation_details.equipment_name,
                    from_user_id,
                    to_user_id,
                    execute_jst,
                    if let Some(n) = note { format!("\n📝 **Note:** {}", n) } else { String::new() }
                ))
                .components(vec![
                    serenity::all::CreateActionRow::Buttons(vec![
                        serenity::all::CreateButton::new(format!("transfer_cancel_{}", transfer_id))
                            .label("🚫 Cancel Transfer")
                            .style(serenity::all::ButtonStyle::Danger),
                    ])
                ])
                .ephemeral(true),
        );
        modal.create_response(&ctx.http, response).await?;

        Ok(())
    }
}
