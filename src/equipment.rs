use anyhow::Result;
use serenity::all::{ChannelId, Context, CreateEmbed, CreateMessage, CreateActionRow, CreateButton, ButtonStyle, Colour, MessageId, EditMessage};
use sqlx::{SqlitePool, Row};
use tracing::{error, info, warn};
use chrono::{Utc, NaiveDateTime};

use crate::models::{Equipment, Tag, Reservation, ManagedMessage};
use crate::time;

/// Equipment visualization and management
pub struct EquipmentRenderer {
    db: SqlitePool,
}

/// Represents an action to take during reconciliation
#[derive(Debug, Clone, PartialEq)]
pub enum EditAction {
    CreateHeader,
    CreateEquipment(i64), // equipment_id
    EditEquipment(i64, i64), // message_id, equipment_id
    DeleteMessage(i64), // message_id
}

/// Plan for reconciling managed messages
#[derive(Debug)]
pub struct EditPlan {
    pub actions: Vec<EditAction>,
    pub creates: usize,
    pub edits: usize, 
    pub deletes: usize,
}

impl EquipmentRenderer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Compute minimal edit plan for reconciling managed messages
    pub fn compute_edit_plan(
        existing_messages: &[ManagedMessage],
        equipment_list: &[(Equipment, Option<Tag>)]
    ) -> EditPlan {
        let mut actions = Vec::new();
        let mut creates = 0;
        let mut edits = 0;
        let mut deletes = 0;

        // Check if header exists (sort_order = 0, message_type = 'Header')
        let has_header = existing_messages.iter().any(|msg| {
            msg.message_type == "Header" && msg.sort_order == Some(0)
        });

        if !has_header {
            actions.push(EditAction::CreateHeader);
            creates += 1;
        }

        // Group existing messages by sort_order (excluding header)
        let mut existing_equipment_messages: Vec<&ManagedMessage> = existing_messages
            .iter()
            .filter(|msg| msg.message_type == "EquipmentEmbed")
            .collect();
        existing_equipment_messages.sort_by_key(|msg| msg.sort_order.unwrap_or(0));

        // Build ideal assignment: equipment messages start at sort_order = 1
        let ideal_count = equipment_list.len();
        let existing_count = existing_equipment_messages.len();

        // Plan message updates for each position
        for (index, (equipment, _tag)) in equipment_list.iter().enumerate() {
            let _target_sort_order = index as i64 + 1; // Start from 1 (header is 0)
            
            if index < existing_count {
                // We have an existing message at this position
                let existing = existing_equipment_messages[index];
                
                // Check if the equipment ID matches
                if existing.equipment_id != Some(equipment.id) {
                    // Equipment changed, need to edit the message
                    actions.push(EditAction::EditEquipment(existing.message_id, equipment.id));
                    edits += 1;
                }
                // If equipment matches, no action needed (content will be updated anyway)
            } else {
                // No existing message, need to create one
                actions.push(EditAction::CreateEquipment(equipment.id));
                creates += 1;
            }
        }

        // Delete excess messages
        for existing in existing_equipment_messages.iter().skip(ideal_count) {
            actions.push(EditAction::DeleteMessage(existing.message_id));
            deletes += 1;
        }

        EditPlan { actions, creates, edits, deletes }
    }

    /// Get equipment with their tags, ordered by tag.sort_order ASC, then equipment.name ASC
    pub async fn get_ordered_equipment(&self, guild_id: i64) -> Result<Vec<(Equipment, Option<Tag>)>> {
        // Get all equipment for the guild
        let equipment_rows = sqlx::query(
            "SELECT id, guild_id, tag_id, name, status, current_location, 
                    unavailable_reason, default_return_location, message_id, 
                    created_at, updated_at
             FROM equipment 
             WHERE guild_id = ?
             ORDER BY name ASC"
        )
        .bind(guild_id)
        .fetch_all(&self.db)
        .await?;

        let mut result = Vec::new();
        
        // For each equipment, get its tag if it has one
        for row in equipment_rows {
            let equipment = Equipment {
                id: row.get("id"),
                guild_id: row.get("guild_id"),
                tag_id: row.get("tag_id"),
                name: row.get("name"),
                status: row.get("status"),
                current_location: row.get("current_location"),
                unavailable_reason: row.get("unavailable_reason"),
                default_return_location: row.get("default_return_location"),
                message_id: row.get("message_id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };

            let tag = if let Some(tag_id) = equipment.tag_id {
                let tag_row = sqlx::query(
                    "SELECT id, guild_id, name, sort_order, created_at FROM tags WHERE id = ?"
                )
                .bind(tag_id)
                .fetch_optional(&self.db)
                .await?;
                
                if let Some(tag_row) = tag_row {
                    Some(Tag {
                        id: tag_row.get("id"),
                        guild_id: tag_row.get("guild_id"),
                        name: tag_row.get("name"),
                        sort_order: tag_row.get("sort_order"),
                        created_at: tag_row.get("created_at"),
                    })
                } else {
                    None
                }
            } else {
                None
            };
            
            result.push((equipment, tag));
        }

        // Sort by tag sort order, then by equipment name
        result.sort_by(|(_, tag_a), (_, tag_b)| {
            let sort_a = tag_a.as_ref().map(|t| t.sort_order).unwrap_or(999999);
            let sort_b = tag_b.as_ref().map(|t| t.sort_order).unwrap_or(999999);
            sort_a.cmp(&sort_b)
        });

        Ok(result)
    }

    /// Create an embed for a single piece of equipment
    pub async fn create_equipment_embed(&self, equipment: &Equipment, tag: &Option<Tag>) -> Result<CreateEmbed> {
        let status_emoji = match equipment.status.as_str() {
            "Available" => "✅",
            "Loaned" => "🔒",
            "Unavailable" => "❌",
            _ => "❓",
        };

        let status_color = match equipment.status.as_str() {
            "Available" => Colour::DARK_GREEN,
            "Loaned" => Colour::ORANGE,
            "Unavailable" => Colour::RED,
            _ => Colour::LIGHT_GREY,
        };

        let mut embed = CreateEmbed::new()
            .title(format!("{} {}", status_emoji, equipment.name))
            .color(status_color)
            .field("Status", &equipment.status, true);

        if let Some(tag) = tag {
            embed = embed.field("Category", &tag.name, true);
        }

        if let Some(location) = &equipment.current_location {
            embed = embed.field("Current Location", location, true);
        } else if let Some(default_location) = &equipment.default_return_location {
            embed = embed.field("Default Return Location", default_location, true);
        }

        if equipment.status == "Unavailable" {
            if let Some(reason) = &equipment.unavailable_reason {
                embed = embed.field("Unavailable Reason", reason, false);
            }
        }

        // Add reservation information
        let current_reservation = self.get_current_or_next_reservation(equipment.id).await?;
        if let Some(reservation) = current_reservation {
            let start_jst = time::utc_to_jst_string(reservation.start_time);
            let end_jst = time::utc_to_jst_string(reservation.end_time);
            let user_mention = format!("<@{}>", reservation.user_id);
            
            let now = Utc::now();
            if reservation.start_time <= now && now < reservation.end_time {
                // Currently reserved
                embed = embed.field("Currently Reserved", format!("By: {}\nUntil: {}", user_mention, end_jst), false);
            } else {
                // Future reservation
                embed = embed.field("Next Reservation", format!("By: {}\nFrom: {} to {}", user_mention, start_jst, end_jst), false);
            }
        } else {
            embed = embed.field("Availability", "Available for reservation", false);
        }

        Ok(embed)
    }

    /// Get the current or next upcoming reservation for equipment
    async fn get_current_or_next_reservation(&self, equipment_id: i64) -> Result<Option<Reservation>> {
        // Use regular query instead of query_as! to handle type conversions manually
        let reservation_row = sqlx::query!(
            "SELECT id, equipment_id, user_id, start_time, end_time, location, status, created_at, updated_at, returned_at, return_location
             FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' AND end_time > CURRENT_TIMESTAMP
             ORDER BY start_time ASC
             LIMIT 1",
            equipment_id
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(row) = reservation_row {
            // Helper function to convert NaiveDateTime to DateTime<Utc>
            let to_utc_datetime = |naive: NaiveDateTime| -> chrono::DateTime<Utc> {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
            };

            Ok(Some(Reservation {
                id: row.id.unwrap_or(0),
                equipment_id: row.equipment_id,
                user_id: row.user_id,
                start_time: to_utc_datetime(row.start_time),
                end_time: to_utc_datetime(row.end_time),
                location: row.location,
                status: row.status,
                created_at: to_utc_datetime(row.created_at.unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc())),
                updated_at: to_utc_datetime(row.updated_at.unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc())),
                returned_at: row.returned_at.map(to_utc_datetime),
                return_location: row.return_location,
            }))
        } else {
            Ok(None)
        }
    }

    /// Create action buttons for equipment
    /// Create the header message with overall management button
    fn create_header_message(&self) -> CreateMessage {
        let embed = CreateEmbed::new()
            .title("🔧 Equipment Management")
            .description("Manage your equipment reservations and organization settings.")
            .color(Colour::BLUE);

        let button = CreateActionRow::Buttons(vec![
            CreateButton::new("overall_mgmt_open")
                .label("📊 Overall Management")
                .style(ButtonStyle::Primary)
        ]);

        CreateMessage::new()
            .embed(embed)
            .components(vec![button])
    }

    pub async fn create_equipment_buttons(&self, equipment: &Equipment) -> Result<Vec<CreateActionRow>> {
        let mut buttons = Vec::new();
        
        // Always show Reserve button for available equipment
        if equipment.status == "Available" {
            buttons.push(
                CreateButton::new(format!("reserve_{}", equipment.id))
                    .label("📅 Reserve")
                    .style(ButtonStyle::Primary)
            );
        }

        // Check if there are any reservations for this equipment that users can edit/cancel
        let user_reservations = sqlx::query!(
            "SELECT id, user_id FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' AND end_time > CURRENT_TIMESTAMP
             ORDER BY start_time ASC",
            equipment.id
        )
        .fetch_all(&self.db)
        .await?;

        // Check/Change button for equipment with active reservations
        if !user_reservations.is_empty() {
            buttons.push(
                CreateButton::new(format!("change_{}", equipment.id))
                    .label("✏️ Check/Change")
                    .style(ButtonStyle::Secondary)
            );

            // Transfer button for equipment with active/upcoming reservations
            // Permission checks will be done in the handler
            buttons.push(
                CreateButton::new(format!("transfer_{}", equipment.id))
                    .label("🔄 Transfer")
                    .style(ButtonStyle::Secondary)
            );

            // Return button for currently loaned equipment
            if equipment.status == "Loaned" {
                buttons.push(
                    CreateButton::new(format!("return_{}", equipment.id))
                        .label("↩️ Return")
                        .style(ButtonStyle::Danger)
                );
            }
        }

        // Admin-only maintenance buttons
        // Note: Permission checking will be done in handlers
        let has_maintenance = self.has_current_or_upcoming_maintenance(equipment.id).await.unwrap_or(false);
        
        if has_maintenance {
            // Show edit/cancel buttons for existing maintenance
            if let Ok(Some(maintenance)) = self.get_current_or_upcoming_maintenance(equipment.id).await {
                buttons.push(
                    CreateButton::new(format!("maint_edit_{}", maintenance.id))
                        .label("🔧 Edit Maintenance")
                        .style(ButtonStyle::Secondary)
                );
                buttons.push(
                    CreateButton::new(format!("maint_cancel_{}", maintenance.id))
                        .label("❌ Cancel Maintenance")
                        .style(ButtonStyle::Danger)
                );
            }
        } else {
            // Show create maintenance button
            buttons.push(
                CreateButton::new(format!("maint_new_{}", equipment.id))
                    .label("🔧 Maintenance")
                    .style(ButtonStyle::Secondary)
            );
        }

        if buttons.is_empty() {
            Ok(vec![])
        } else {
            Ok(vec![CreateActionRow::Buttons(buttons)])
        }
    }

    /// Render or update all equipment embeds in the channel using minimal edits
    pub async fn reconcile_equipment_display(
        &self,
        ctx: &Context,
        guild_id: i64,
        channel_id: i64,
    ) -> Result<()> {
        info!("Reconciling equipment display for guild {} in channel {}", guild_id, channel_id);

        let channel = ChannelId::new(channel_id as u64);

        // Get current equipment ordered by tag and name
        let equipment_list = self.get_ordered_equipment(guild_id).await?;

        // Get all existing managed messages for this channel (including header)
        let existing_messages: Vec<ManagedMessage> = sqlx::query(
            "SELECT id, guild_id, channel_id, message_id, message_type, equipment_id, sort_order, created_at
             FROM managed_messages 
             WHERE guild_id = ? AND channel_id = ?
             ORDER BY sort_order ASC"
        )
        .bind(guild_id)
        .bind(channel_id)
        .fetch_all(&self.db)
        .await?
        .into_iter()
        .map(|row| {
            let created_at = if let Ok(naive) = row.try_get::<NaiveDateTime, _>("created_at") {
                chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
            } else {
                chrono::Utc::now()
            };
            
            ManagedMessage {
                id: row.get("id"),
                guild_id: row.get("guild_id"),
                channel_id: row.get("channel_id"),
                message_id: row.get("message_id"),
                message_type: row.get("message_type"),
                equipment_id: row.get("equipment_id"),
                sort_order: row.get("sort_order"),
                created_at,
            }
        })
        .collect();

        // Compute minimal edit plan
        let edit_plan = Self::compute_edit_plan(&existing_messages, &equipment_list);
        
        info!(
            "Edit plan: {} creates, {} edits, {} deletes",
            edit_plan.creates, edit_plan.edits, edit_plan.deletes
        );

        // Execute the edit plan
        for action in &edit_plan.actions {
            match action {
                EditAction::CreateHeader => {
                    let message_builder = self.create_header_message();
                    match channel.send_message(&ctx.http, message_builder).await {
                        Ok(message) => {
                            let message_id = message.id.get() as i64;
                            sqlx::query(
                                "INSERT INTO managed_messages 
                                 (guild_id, channel_id, message_id, message_type, equipment_id, sort_order)
                                 VALUES (?, ?, ?, 'Header', NULL, 0)"
                            )
                            .bind(guild_id)
                            .bind(channel_id)
                            .bind(message_id)
                            .execute(&self.db)
                            .await?;
                            info!("Created header message");
                        }
                        Err(e) => {
                            error!("Failed to create header message: {}", e);
                        }
                    }
                }
                
                EditAction::CreateEquipment(equipment_id) => {
                    // Find the equipment and its tag
                    if let Some((equipment, tag)) = equipment_list.iter().find(|(eq, _)| eq.id == *equipment_id) {
                        let embed = self.create_equipment_embed(equipment, tag).await?;
                        let buttons = self.create_equipment_buttons(equipment).await?;
                        
                        let mut message_builder = CreateMessage::new().embed(embed);
                        if !buttons.is_empty() {
                            message_builder = message_builder.components(buttons);
                        }
                        
                        match channel.send_message(&ctx.http, message_builder).await {
                            Ok(message) => {
                                let message_id = message.id.get() as i64;
                                // Calculate sort_order (equipment messages start from 1)
                                let sort_order = equipment_list.iter().position(|(eq, _)| eq.id == *equipment_id).unwrap() as i64 + 1;
                                
                                sqlx::query(
                                    "INSERT INTO managed_messages 
                                     (guild_id, channel_id, message_id, message_type, equipment_id, sort_order)
                                     VALUES (?, ?, ?, 'EquipmentEmbed', ?, ?)"
                                )
                                .bind(guild_id)
                                .bind(channel_id)
                                .bind(message_id)
                                .bind(equipment_id)
                                .bind(sort_order)
                                .execute(&self.db)
                                .await?;
                                
                                info!("Created equipment embed for {} (ID: {})", equipment.name, equipment.id);
                            }
                            Err(e) => {
                                error!("Failed to create equipment embed for {}: {}", equipment.name, e);
                            }
                        }
                    }
                }
                
                EditAction::EditEquipment(message_id, equipment_id) => {
                    // Find the equipment and its tag
                    if let Some((equipment, tag)) = equipment_list.iter().find(|(eq, _)| eq.id == *equipment_id) {
                        let embed = self.create_equipment_embed(equipment, tag).await?;
                        let buttons = self.create_equipment_buttons(equipment).await?;
                        
                        let mut edit_builder = EditMessage::new().embed(embed);
                        if !buttons.is_empty() {
                            edit_builder = edit_builder.components(buttons);
                        } else {
                            edit_builder = edit_builder.components(Vec::new());
                        }
                        
                        match channel.edit_message(&ctx.http, MessageId::new(*message_id as u64), edit_builder).await {
                            Ok(_) => {
                                // Update the equipment_id in the database
                                sqlx::query(
                                    "UPDATE managed_messages SET equipment_id = ? WHERE message_id = ?"
                                )
                                .bind(equipment_id)
                                .bind(message_id)
                                .execute(&self.db)
                                .await?;
                                
                                info!("Updated equipment embed for {} (ID: {})", equipment.name, equipment.id);
                            }
                            Err(e) => {
                                warn!("Failed to edit equipment message {}: {}", message_id, e);
                            }
                        }
                    }
                }
                
                EditAction::DeleteMessage(message_id) => {
                    if let Err(e) = channel.delete_message(&ctx.http, MessageId::new(*message_id as u64)).await {
                        warn!("Failed to delete message {}: {}", message_id, e);
                    }
                    
                    // Remove from database
                    sqlx::query(
                        "DELETE FROM managed_messages WHERE message_id = ?"
                    )
                    .bind(message_id)
                    .execute(&self.db)
                    .await?;
                    
                    info!("Deleted message {}", message_id);
                }
            }
        }

        info!(
            "Equipment display reconciliation completed. {} creates, {} edits, {} deletes.",
            edit_plan.creates, edit_plan.edits, edit_plan.deletes
        );
        Ok(())
    }

    /// Remove duplicate guide messages if present, keeping only one
    pub async fn cleanup_duplicate_guides(
        &self,
        ctx: &Context,
        guild_id: i64,
        channel_id: i64,
    ) -> Result<()> {
        let channel = ChannelId::new(channel_id as u64);

        // Get all guide messages
        let guide_messages: Vec<(i64, i64)> = sqlx::query_as(
            "SELECT id, message_id FROM managed_messages 
             WHERE guild_id = ? AND channel_id = ? AND message_type = 'Guide'
             ORDER BY id ASC"
        )
        .bind(guild_id)
        .bind(channel_id)
        .fetch_all(&self.db)
        .await?;

        // If there are multiple guide messages, delete all but the first
        if guide_messages.len() > 1 {
            info!("Found {} guide messages, removing duplicates", guide_messages.len());
            
            for guide in guide_messages.iter().skip(1) {
                // Delete from Discord
                if let Err(e) = channel.delete_message(&ctx.http, MessageId::new(guide.1 as u64)).await {
                    warn!("Failed to delete duplicate guide message {}: {}", guide.1, e);
                }

                // Delete from database
                sqlx::query("DELETE FROM managed_messages WHERE id = ?")
                    .bind(guide.0)
                    .execute(&self.db)
                    .await?;
            }
        }

        Ok(())
    }

    /// Check if equipment has current or upcoming maintenance
    async fn has_current_or_upcoming_maintenance(&self, equipment_id: i64) -> Result<bool> {
        let now_utc = Utc::now().naive_utc();

        let count = sqlx::query!(
            "SELECT COUNT(*) as count FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND end_utc > ?",
            equipment_id,
            now_utc
        )
        .fetch_one(&self.db)
        .await?;

        Ok(count.count > 0)
    }

    /// Get current or next upcoming maintenance window for equipment
    async fn get_current_or_upcoming_maintenance(&self, equipment_id: i64) -> Result<Option<crate::models::MaintenanceWindow>> {
        let now_utc = Utc::now().naive_utc();

        let maintenance_row = sqlx::query!(
            "SELECT id, equipment_id, start_utc, end_utc, reason, created_by_user_id, created_at_utc, canceled_at_utc, canceled_by_user_id
             FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND end_utc > ?
             ORDER BY start_utc ASC
             LIMIT 1",
            equipment_id,
            now_utc
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(row) = maintenance_row {
            use crate::models::MaintenanceWindow;
            let maintenance = MaintenanceWindow {
                id: row.id,
                equipment_id: row.equipment_id,
                start_utc: to_utc_datetime(row.start_utc),
                end_utc: to_utc_datetime(row.end_utc),
                reason: row.reason,
                created_by_user_id: row.created_by_user_id,
                created_at_utc: to_utc_datetime(row.created_at_utc.unwrap_or_else(|| chrono::Utc::now().naive_utc())),
                canceled_at_utc: row.canceled_at_utc.map(to_utc_datetime),
                canceled_by_user_id: row.canceled_by_user_id,
            };
            Ok(Some(maintenance))
        } else {
            Ok(None)
        }
    }
}

// Helper function to convert NaiveDateTime to DateTime<Utc>
fn to_utc_datetime(naive: chrono::NaiveDateTime) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::<chrono::Utc>::from_naive_utc_and_offset(naive, chrono::Utc)
}