use anyhow::Result;
use serenity::all::{ChannelId, Context, CreateEmbed, CreateMessage, Colour, MessageId};
use sqlx::{SqlitePool, Row};
use tracing::{error, info, warn};

use crate::models::{Equipment, Tag};

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
    pub fn create_equipment_embed(&self, equipment: &Equipment, tag: &Option<Tag>) -> CreateEmbed {
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

        embed
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
            let embed = self.create_equipment_embed(equipment, tag);
            
            match channel.send_message(&ctx.http, CreateMessage::new().embed(embed)).await {
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