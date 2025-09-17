use anyhow::Result;
use serenity::all::{ChannelId, Context, CreateEmbed, CreateMessage, CreateActionRow, CreateButton, ButtonStyle, Colour, MessageId};
use sqlx::{SqlitePool, Row};
use tracing::{error, info, warn};
use chrono::{Utc, NaiveDateTime};

use crate::models::{Equipment, Tag, Reservation};
use crate::time;

/// Equipment visualization and management
pub struct EquipmentRenderer {
    db: SqlitePool,
}

impl EquipmentRenderer {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
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
            "SELECT id, equipment_id, user_id, start_time, end_time, location, status, created_at, updated_at
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
            }))
        } else {
            Ok(None)
        }
    }

    /// Create action buttons for equipment
    async fn create_equipment_buttons(&self, equipment: &Equipment) -> Result<Vec<CreateActionRow>> {
        let mut buttons = Vec::new();
        
        // Always show Reserve button for available equipment
        if equipment.status == "Available" {
            buttons.push(
                CreateButton::new(format!("eq_reserve:{}", equipment.id))
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

        // Add edit/cancel buttons for existing reservations (will be filtered by permissions in handlers)
        for reservation in user_reservations {
            if let Some(reservation_id) = reservation.id {
                buttons.push(
                    CreateButton::new(format!("res_edit:{}", reservation_id))
                        .label("✏️ Edit")
                        .style(ButtonStyle::Secondary)
                );
                buttons.push(
                    CreateButton::new(format!("res_cancel:{}", reservation_id))
                        .label("❌ Cancel")
                        .style(ButtonStyle::Danger)
                );
                // Only show buttons for first reservation to avoid clutter
                break;
            }
        }

        if buttons.is_empty() {
            Ok(vec![])
        } else {
            Ok(vec![CreateActionRow::Buttons(buttons)])
        }
    }

    /// Render or update all equipment embeds in the channel
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

        // Get existing managed messages for equipment embeds
        let existing_messages: Vec<(i64, Option<i64>)> = sqlx::query_as(
            "SELECT message_id, equipment_id FROM managed_messages 
             WHERE guild_id = ? AND channel_id = ? AND message_type = 'EquipmentEmbed'
             ORDER BY sort_order ASC"
        )
        .bind(guild_id)
        .bind(channel_id)
        .fetch_all(&self.db)
        .await?;

        // Delete existing equipment embed messages from Discord
        for existing in &existing_messages {
            if let Err(e) = channel.delete_message(&ctx.http, MessageId::new(existing.0 as u64)).await {
                warn!("Failed to delete existing equipment message {}: {}", existing.0, e);
            }
        }

        // Delete existing equipment embed records from database
        sqlx::query(
            "DELETE FROM managed_messages 
             WHERE guild_id = ? AND channel_id = ? AND message_type = 'EquipmentEmbed'"
        )
        .bind(guild_id)
        .bind(channel_id)
        .execute(&self.db)
        .await?;

        // Create new equipment embeds
        for (sort_order, (equipment, tag)) in equipment_list.iter().enumerate() {
            let embed = self.create_equipment_embed(equipment, tag).await?;
            
            // Create buttons for equipment actions
            let buttons = self.create_equipment_buttons(equipment).await?;
            
            let mut message_builder = CreateMessage::new().embed(embed);
            if !buttons.is_empty() {
                message_builder = message_builder.components(buttons);
            }
            
            match channel.send_message(&ctx.http, message_builder).await {
                Ok(message) => {
                    // Save the message reference
                    let message_id = message.id.get() as i64;
                    let equipment_id = equipment.id;
                    let sort_order_i64 = sort_order as i64;
                    
                    sqlx::query(
                        "INSERT INTO managed_messages 
                         (guild_id, channel_id, message_id, message_type, equipment_id, sort_order) 
                         VALUES (?, ?, ?, ?, ?, ?)"
                    )
                    .bind(guild_id)
                    .bind(channel_id)
                    .bind(message_id)
                    .bind("EquipmentEmbed")
                    .bind(equipment_id)
                    .bind(sort_order_i64)
                    .execute(&self.db)
                    .await?;

                    info!("Created equipment embed for {} (ID: {})", equipment.name, equipment.id);
                }
                Err(e) => {
                    error!("Failed to create equipment embed for {}: {}", equipment.name, e);
                }
            }
        }

        info!("Equipment display reconciliation completed. Created {} embeds.", equipment_list.len());
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
}