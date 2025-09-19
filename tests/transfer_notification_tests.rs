use anyhow::Result;
use oucc_kizai_bot::transfer_notifications::{TransferNotificationService, TransferNotificationType};
use oucc_kizai_bot::traits::DiscordApi;
use serenity::all::{ChannelId, Message, UserId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

mod common;

/// Mock Discord API for testing
#[derive(Debug, Default, Clone)]
pub struct MockDiscordApi {
    sent_dms: Arc<Mutex<Vec<(UserId, String)>>>,
    channel_messages: Arc<Mutex<Vec<(ChannelId, String)>>>,
    dm_failure_mode: Arc<Mutex<bool>>,
}

impl MockDiscordApi {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn set_dm_failure_mode(&self, enabled: bool) {
        *self.dm_failure_mode.lock().await = enabled;
    }

    pub async fn get_sent_dms(&self) -> Vec<(UserId, String)> {
        self.sent_dms.lock().await.clone()
    }

    pub async fn get_channel_messages(&self) -> Vec<(ChannelId, String)> {
        self.channel_messages.lock().await.clone()
    }

    pub async fn clear(&self) {
        self.sent_dms.lock().await.clear();
        self.channel_messages.lock().await.clear();
    }
}

#[async_trait::async_trait]
impl DiscordApi for MockDiscordApi {
    async fn send_dm(&self, user_id: UserId, message: &str) -> Result<Option<Message>> {
        if *self.dm_failure_mode.lock().await {
            return Ok(None); // Simulate DM failure
        }

        self.sent_dms.lock().await.push((user_id, message.to_string()));
        // Return a mock message (the actual Message type is complex to construct)
        Ok(Some(unsafe { std::mem::zeroed() }))
    }

    async fn send_channel_message(&self, channel_id: ChannelId, message: &str) -> Result<Message> {
        self.channel_messages.lock().await.push((channel_id, message.to_string()));
        // Return a mock message
        Ok(unsafe { std::mem::zeroed() })
    }
}

#[tokio::test]
async fn test_transfer_notification_dm_success() -> Result<()> {
    let db = common::setup_test_db().await?;
    let mock_api = MockDiscordApi::new();
    let service = TransferNotificationService::new(db.clone());

    // Create test guild
    let guild = common::GuildBuilder::new(12345)
        .with_reservation_channel(67890)
        .build(&db)
        .await?;

    // Create test equipment
    let equipment = common::EquipmentBuilder::new(guild.id, "Test Camera".to_string())
        .build(&db)
        .await?;

    let notification = TransferNotificationType::Approved {
        equipment_name: "Test Camera".to_string(),
    };

    let result = service.send_notification_with_api(
        &mock_api,
        123456789, // user_id
        1,         // reservation_id
        equipment.id,
        guild.id,
        notification,
    ).await?;

    // Should succeed via DM
    assert!(matches!(result, oucc_kizai_bot::transfer_notifications::DeliveryMethod::Dm));
    
    let sent_dms = mock_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert!(sent_dms[0].1.contains("移譲承認通知"));
    assert!(sent_dms[0].1.contains("Test Camera"));

    Ok(())
}

#[tokio::test]
async fn test_transfer_notification_dm_failure_fallback() -> Result<()> {
    let db = common::setup_test_db().await?;
    let mock_api = MockDiscordApi::new();
    let service = TransferNotificationService::new(db.clone());

    // Enable DM failure mode
    mock_api.set_dm_failure_mode(true).await;

    // Create test guild with fallback enabled
    let guild = common::GuildBuilder::new(12345)
        .with_reservation_channel(67890)
        .with_dm_fallback_enabled(true)
        .build(&db)
        .await?;

    // Create test equipment
    let equipment = common::EquipmentBuilder::new(guild.id, "Test Camera".to_string())
        .build(&db)
        .await?;

    let notification = TransferNotificationType::RequestSent {
        equipment_name: "Test Camera".to_string(),
        requester_id: 555555555,
        reservation_id: 1,
    };

    let result = service.send_notification_with_api(
        &mock_api,
        123456789, // user_id
        1,         // reservation_id
        equipment.id,
        guild.id,
        notification,
    ).await?;

    // Should fall back to channel
    assert!(matches!(result, oucc_kizai_bot::transfer_notifications::DeliveryMethod::Channel));
    
    // No DMs should be sent
    let sent_dms = mock_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 0);

    // Should have channel message
    let channel_messages = mock_api.get_channel_messages().await;
    assert_eq!(channel_messages.len(), 1);
    assert!(channel_messages[0].1.contains("<@123456789>"));
    assert!(channel_messages[0].1.contains("Test Camera"));
    assert!(channel_messages[0].1.contains("予約ID: #1"));
    // Should NOT contain sensitive details
    assert!(!channel_messages[0].1.contains("期間"));
    assert!(!channel_messages[0].1.contains("場所"));

    Ok(())
}

#[tokio::test]
async fn test_transfer_notification_complete_failure() -> Result<()> {
    let db = common::setup_test_db().await?;
    let mock_api = MockDiscordApi::new();
    let service = TransferNotificationService::new(db.clone());

    // Enable DM failure mode
    mock_api.set_dm_failure_mode(true).await;

    // Create test guild with no fallback channel
    let guild = common::GuildBuilder::new(12345)
        .build(&db)
        .await?;

    // Create test equipment
    let equipment = common::EquipmentBuilder::new(guild.id, "Test Camera".to_string())
        .build(&db)
        .await?;

    let notification = TransferNotificationType::Denied {
        equipment_name: "Test Camera".to_string(),
        reason: "User declined".to_string(),
    };

    let result = service.send_notification_with_api(
        &mock_api,
        123456789, // user_id
        1,         // reservation_id
        equipment.id,
        guild.id,
        notification,
    ).await?;

    // Should fail completely
    assert!(matches!(result, oucc_kizai_bot::transfer_notifications::DeliveryMethod::Failed));
    
    // Check that failure was logged to equipment_logs
    let log_entry = sqlx::query!(
        "SELECT * FROM equipment_logs WHERE equipment_id = ? AND action = 'NotifyFail'",
        equipment.id
    )
    .fetch_optional(&db)
    .await?;

    assert!(log_entry.is_some());
    let log = log_entry.unwrap();
    assert!(log.notes.unwrap_or_default().contains("Denial notification"));
    assert!(log.notes.unwrap_or_default().contains("Test Camera"));

    Ok(())
}

#[tokio::test]
async fn test_notification_message_content() -> Result<()> {
    // Test that notification messages contain appropriate content
    
    let approved = TransferNotificationType::Approved {
        equipment_name: "Camera A".to_string(),
    };
    assert!(approved.dm_message().contains("移譲承認通知"));
    assert!(approved.dm_message().contains("Camera A"));

    let denied = TransferNotificationType::Denied {
        equipment_name: "Camera A".to_string(),
        reason: "Unavailable".to_string(),
    };
    assert!(denied.dm_message().contains("移譲拒否通知"));
    assert!(denied.dm_message().contains("Camera A"));
    assert!(denied.dm_message().contains("Unavailable"));

    let expired = TransferNotificationType::Expired {
        equipment_name: "Camera A".to_string(),
    };
    assert!(expired.dm_message().contains("移譲期限切れ通知"));
    assert!(expired.dm_message().contains("Camera A"));
    assert!(expired.dm_message().contains("3時間"));

    let request = TransferNotificationType::RequestSent {
        equipment_name: "Camera A".to_string(),
        requester_id: 123,
        reservation_id: 456,
    };
    assert!(request.dm_message().contains("予約移譲依頼"));
    assert!(request.dm_message().contains("Camera A"));

    // Test fallback messages don't contain sensitive info
    assert!(!request.fallback_message(456).contains("期間"));
    assert!(!request.fallback_message(456).contains("場所"));
    assert!(request.fallback_message(456).contains("Camera A"));
    assert!(request.fallback_message(456).contains("予約ID: #456"));

    Ok(())
}