use anyhow::Result;
use chrono::Utc;
use serenity::all::{ChannelId, Context, UserId};
use sqlx::SqlitePool;
use tracing::{error, info, warn};

use crate::traits::DiscordApi;

/// Delivery method for notifications
#[derive(Debug, Clone)]
pub enum DeliveryMethod {
    Dm,
    Channel,
    Failed,
}

impl From<DeliveryMethod> for String {
    fn from(method: DeliveryMethod) -> String {
        match method {
            DeliveryMethod::Dm => "DM".to_string(),
            DeliveryMethod::Channel => "Channel".to_string(),
            DeliveryMethod::Failed => "Failed".to_string(),
        }
    }
}

/// Transfer notification types with their templates
#[derive(Debug, Clone)]
pub enum TransferNotificationType {
    /// Transfer request sent to target user
    RequestSent { equipment_name: String, requester_id: i64, reservation_id: i64 },
    /// Transfer approved by target
    Approved { equipment_name: String },
    /// Transfer denied by target
    Denied { equipment_name: String, reason: String },
    /// Transfer cancelled by requester or admin
    Cancelled { equipment_name: String, canceller_id: i64 },
    /// Transfer expired due to timeout
    Expired { equipment_name: String },
}

impl TransferNotificationType {
    /// Get the DM message content for this notification type
    pub fn dm_message(&self) -> String {
        match self {
            TransferNotificationType::RequestSent { equipment_name, requester_id, .. } => {
                format!(
                    "📤 **予約移譲依頼**\n\n<@{}>から「{}」の予約移譲依頼があります。\n\nDMで詳細を確認して承認・拒否を選択してください。\n\n⚠️ この依頼は3時間後に自動的に期限切れになります。",
                    requester_id, equipment_name
                )
            }
            TransferNotificationType::Approved { equipment_name } => {
                format!("✅ **移譲承認通知**\n\n「{}」の予約移譲依頼が承認されました。", equipment_name)
            }
            TransferNotificationType::Denied { equipment_name, reason } => {
                format!(
                    "❌ **移譲拒否通知**\n\n「{}」の予約移譲依頼が拒否されました。\n\n理由: {}",
                    equipment_name, reason
                )
            }
            TransferNotificationType::Cancelled { equipment_name, .. } => {
                format!("🚫 **移譲キャンセル通知**\n\n「{}」の予約移譲依頼がキャンセルされました。", equipment_name)
            }
            TransferNotificationType::Expired { equipment_name } => {
                format!(
                    "⏰ **移譲期限切れ通知**\n\n「{}」の予約移譲依頼が3時間以内に承認されなかったため、自動的にキャンセルされました。",
                    equipment_name
                )
            }
        }
    }

    /// Get the generic public fallback message (no sensitive details)
    pub fn fallback_message(&self, reservation_id: i64) -> String {
        match self {
            TransferNotificationType::RequestSent { equipment_name, .. } => {
                format!(
                    "「{}」の予約移譲に関する通知があります。予約ID: #{}\n\nDMを有効にして詳細を確認してください。",
                    equipment_name, reservation_id
                )
            }
            TransferNotificationType::Approved { equipment_name } => {
                format!("「{}」の予約移譲に関する更新があります。予約ID: #{}", equipment_name, reservation_id)
            }
            TransferNotificationType::Denied { equipment_name, .. } => {
                format!("「{}」の予約移譲に関する更新があります。予約ID: #{}", equipment_name, reservation_id)
            }
            TransferNotificationType::Cancelled { equipment_name, .. } => {
                format!("「{}」の予約移譲に関する更新があります。予約ID: #{}", equipment_name, reservation_id)
            }
            TransferNotificationType::Expired { equipment_name } => {
                format!("「{}」の予約移譲に関する更新があります。予約ID: #{}", equipment_name, reservation_id)
            }
        }
    }

    /// Get the equipment name for logging purposes
    pub fn equipment_name(&self) -> &str {
        match self {
            TransferNotificationType::RequestSent { equipment_name, .. } |
            TransferNotificationType::Approved { equipment_name } |
            TransferNotificationType::Denied { equipment_name, .. } |
            TransferNotificationType::Cancelled { equipment_name, .. } |
            TransferNotificationType::Expired { equipment_name } => equipment_name,
        }
    }
}

/// Centralized transfer notification system
pub struct TransferNotificationService {
    db: SqlitePool,
}

impl TransferNotificationService {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Send a transfer notification with DM-first delivery and fallback
    pub async fn send_notification(
        &self,
        ctx: &Context,
        user_id: i64,
        reservation_id: i64,
        equipment_id: i64,
        guild_id: i64,
        notification: TransferNotificationType,
    ) -> Result<DeliveryMethod> {
        let user_id_discord = UserId::new(user_id as u64);
        let dm_message = notification.dm_message();

        // Try sending DM first
        match self.try_send_dm(ctx, user_id_discord, &dm_message).await {
            Ok(true) => return Ok(DeliveryMethod::Dm),
            Ok(false) => {
                info!("DM failed for user {}, attempting fallback", user_id);
            }
            Err(e) => {
                warn!("Error sending DM to user {}: {}", user_id, e);
            }
        }

        // Get guild configuration for fallback
        let guild_config = sqlx::query!(
            "SELECT reservation_channel_id, dm_fallback_channel_enabled FROM guilds WHERE id = ?",
            guild_id
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(config) = guild_config {
            if config.dm_fallback_channel_enabled.unwrap_or(true) {
                if let Some(channel_id) = config.reservation_channel_id {
                    let fallback_message = notification.fallback_message(reservation_id);
                    let channel_message = format!("<@{}> {}", user_id, fallback_message);
                    
                    match self.try_send_channel_message(ctx, channel_id, &channel_message).await {
                        Ok(true) => return Ok(DeliveryMethod::Channel),
                        Ok(false) => {
                            warn!("Channel fallback failed for channel {}", channel_id);
                        }
                        Err(e) => {
                            warn!("Error sending channel fallback message: {}", e);
                        }
                    }
                }
            }
        }

        // Both delivery methods failed - log the failure
        self.log_notification_failure(equipment_id, user_id, &notification).await?;
        
        Ok(DeliveryMethod::Failed)
    }

    /// Send notification using DiscordApi trait (for job worker)
    pub async fn send_notification_with_api(
        &self,
        discord_api: &dyn DiscordApi,
        user_id: i64,
        reservation_id: i64,
        equipment_id: i64,
        guild_id: i64,
        notification: TransferNotificationType,
    ) -> Result<DeliveryMethod> {
        let user_id_discord = UserId::new(user_id as u64);
        let dm_message = notification.dm_message();

        // Try sending DM first
        match discord_api.send_dm(user_id_discord, &dm_message).await {
            Ok(Some(_)) => return Ok(DeliveryMethod::Dm),
            Ok(None) => {
                info!("DM failed for user {}, attempting fallback", user_id);
            }
            Err(e) => {
                warn!("Error sending DM to user {}: {}", user_id, e);
            }
        }

        // Get guild configuration for fallback
        let guild_config = sqlx::query!(
            "SELECT reservation_channel_id, dm_fallback_channel_enabled FROM guilds WHERE id = ?",
            guild_id
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(config) = guild_config {
            if config.dm_fallback_channel_enabled.unwrap_or(true) {
                if let Some(channel_id) = config.reservation_channel_id {
                    let fallback_message = notification.fallback_message(reservation_id);
                    let channel_message = format!("<@{}> {}", user_id, fallback_message);
                    let channel_id_discord = ChannelId::new(channel_id as u64);
                    
                    match discord_api.send_channel_message(channel_id_discord, &channel_message).await {
                        Ok(_) => return Ok(DeliveryMethod::Channel),
                        Err(e) => {
                            warn!("Error sending channel fallback message: {}", e);
                        }
                    }
                }
            }
        }

        // Both delivery methods failed - log the failure
        self.log_notification_failure(equipment_id, user_id, &notification).await?;
        
        Ok(DeliveryMethod::Failed)
    }

    /// Try to send a DM to the user
    async fn try_send_dm(&self, ctx: &Context, user_id: UserId, message: &str) -> Result<bool> {
        match user_id.create_dm_channel(&ctx.http).await {
            Ok(dm_channel) => {
                match dm_channel
                    .send_message(&ctx.http, serenity::all::CreateMessage::new().content(message))
                    .await
                {
                    Ok(_) => Ok(true),
                    Err(_) => Ok(false), // DM failed, user likely has DMs disabled
                }
            }
            Err(_) => Ok(false), // Failed to create DM channel
        }
    }

    /// Try to send a message to a channel
    async fn try_send_channel_message(&self, ctx: &Context, channel_id: i64, message: &str) -> Result<bool> {
        let channel_id_discord = ChannelId::new(channel_id as u64);
        
        match channel_id_discord
            .send_message(&ctx.http, serenity::all::CreateMessage::new().content(message))
            .await
        {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }

    /// Log notification failure to equipment_logs
    async fn log_notification_failure(
        &self,
        equipment_id: i64,
        user_id: i64,
        notification: &TransferNotificationType,
    ) -> Result<()> {
        let note = format!(
            "Transfer notification delivery failed: {} for equipment '{}'",
            match notification {
                TransferNotificationType::RequestSent { .. } => "Request notification",
                TransferNotificationType::Approved { .. } => "Approval notification",
                TransferNotificationType::Denied { .. } => "Denial notification",
                TransferNotificationType::Cancelled { .. } => "Cancellation notification",
                TransferNotificationType::Expired { .. } => "Expiration notification",
            },
            notification.equipment_name()
        );

        let timestamp = Utc::now();
        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'NotifyFail', NULL, NULL, NULL, ?, ?)",
            equipment_id,
            user_id,
            note,
            timestamp
        )
        .execute(&self.db)
        .await?;

        error!("Logged notification failure for equipment {} user {}: {}", equipment_id, user_id, note);

        Ok(())
    }
}