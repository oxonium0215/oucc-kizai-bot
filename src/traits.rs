use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serenity::model::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Trait for Discord API operations to enable mocking
#[async_trait]
pub trait DiscordApi: Send + Sync {
    /// Send a direct message to a user
    async fn send_dm(&self, user_id: UserId, content: &str) -> Result<Option<MessageId>>;

    /// Send a message to a channel
    async fn send_channel_message(&self, channel_id: ChannelId, content: &str)
        -> Result<MessageId>;

    /// Edit a message
    async fn edit_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
        new_content: &str,
    ) -> Result<()>;

    /// Delete a message
    async fn delete_message(&self, channel_id: ChannelId, message_id: MessageId) -> Result<()>;

    /// Record a button interaction response
    async fn respond_to_interaction(&self, interaction_id: &str, response: &str) -> Result<()>;
}

/// Trait for clock operations to enable deterministic testing
#[async_trait]
pub trait Clock: Send + Sync {
    /// Get current UTC time
    fn now_utc(&self) -> DateTime<Utc>;

    /// Sleep until a specific time
    async fn sleep_until(&self, target: DateTime<Utc>);

    /// Sleep for a duration (convenience method)
    async fn sleep_duration(&self, duration: chrono::Duration) {
        let target = self.now_utc() + duration;
        self.sleep_until(target).await;
    }
}

/// Production implementation using system clock
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now_utc(&self) -> DateTime<Utc> {
        Utc::now()
    }

    async fn sleep_until(&self, target: DateTime<Utc>) {
        let now = Utc::now();
        if target > now {
            let duration = target - now;
            if let Ok(std_duration) = duration.to_std() {
                tokio::time::sleep(std_duration).await;
            }
        }
    }
}

/// Mock implementation for testing
#[derive(Debug, Clone)]
pub struct MockDiscordApi {
    pub sent_dms: Arc<Mutex<Vec<(UserId, String)>>>,
    pub channel_messages: Arc<Mutex<Vec<(ChannelId, String)>>>,
    pub edited_messages: Arc<Mutex<Vec<(ChannelId, MessageId, String)>>>,
    pub deleted_messages: Arc<Mutex<Vec<(ChannelId, MessageId)>>>,
    pub interaction_responses: Arc<Mutex<Vec<(String, String)>>>,
    pub dm_failure_mode: Arc<Mutex<bool>>, // Simulate DM failures
}

impl MockDiscordApi {
    pub fn new() -> Self {
        Self {
            sent_dms: Arc::new(Mutex::new(Vec::new())),
            channel_messages: Arc::new(Mutex::new(Vec::new())),
            edited_messages: Arc::new(Mutex::new(Vec::new())),
            deleted_messages: Arc::new(Mutex::new(Vec::new())),
            interaction_responses: Arc::new(Mutex::new(Vec::new())),
            dm_failure_mode: Arc::new(Mutex::new(false)),
        }
    }

    /// Enable DM failure simulation
    pub async fn set_dm_failure_mode(&self, enabled: bool) {
        *self.dm_failure_mode.lock().await = enabled;
    }

    /// Get all sent DMs
    pub async fn get_sent_dms(&self) -> Vec<(UserId, String)> {
        self.sent_dms.lock().await.clone()
    }

    /// Get all channel messages
    pub async fn get_channel_messages(&self) -> Vec<(ChannelId, String)> {
        self.channel_messages.lock().await.clone()
    }

    /// Clear all recorded interactions
    pub async fn clear(&self) {
        self.sent_dms.lock().await.clear();
        self.channel_messages.lock().await.clear();
        self.edited_messages.lock().await.clear();
        self.deleted_messages.lock().await.clear();
        self.interaction_responses.lock().await.clear();
    }
}

#[async_trait]
impl DiscordApi for MockDiscordApi {
    async fn send_dm(&self, user_id: UserId, content: &str) -> Result<Option<MessageId>> {
        if *self.dm_failure_mode.lock().await {
            // Simulate DM failure (user has DMs disabled, etc.)
            return Ok(None);
        }

        self.sent_dms
            .lock()
            .await
            .push((user_id, content.to_string()));
        // Return a mock message ID
        Ok(Some(MessageId::new(12345)))
    }

    async fn send_channel_message(
        &self,
        channel_id: ChannelId,
        content: &str,
    ) -> Result<MessageId> {
        self.channel_messages
            .lock()
            .await
            .push((channel_id, content.to_string()));
        // Return a mock message ID
        Ok(MessageId::new(67890))
    }

    async fn edit_message(
        &self,
        channel_id: ChannelId,
        message_id: MessageId,
        new_content: &str,
    ) -> Result<()> {
        self.edited_messages
            .lock()
            .await
            .push((channel_id, message_id, new_content.to_string()));
        Ok(())
    }

    async fn delete_message(&self, channel_id: ChannelId, message_id: MessageId) -> Result<()> {
        self.deleted_messages
            .lock()
            .await
            .push((channel_id, message_id));
        Ok(())
    }

    async fn respond_to_interaction(&self, interaction_id: &str, response: &str) -> Result<()> {
        self.interaction_responses
            .lock()
            .await
            .push((interaction_id.to_string(), response.to_string()));
        Ok(())
    }
}

/// Test clock implementation for deterministic time control
#[derive(Debug, Clone)]
pub struct TestClock {
    current_time: Arc<Mutex<DateTime<Utc>>>,
}

impl TestClock {
    pub fn new(initial_time: DateTime<Utc>) -> Self {
        Self {
            current_time: Arc::new(Mutex::new(initial_time)),
        }
    }

    /// Advance the clock by a specific duration
    pub async fn advance(&self, duration: chrono::Duration) {
        let mut time = self.current_time.lock().await;
        *time = *time + duration;
    }

    /// Set the clock to a specific time
    pub async fn set_time(&self, new_time: DateTime<Utc>) {
        let mut time = self.current_time.lock().await;
        *time = new_time;
    }
}

#[async_trait]
impl Clock for TestClock {
    fn now_utc(&self) -> DateTime<Utc> {
        // Use a blocking operation here since tokio async functions
        // can't be called from non-async contexts
        futures::executor::block_on(async { *self.current_time.lock().await })
    }

    async fn sleep_until(&self, target: DateTime<Utc>) {
        let current = *self.current_time.lock().await;
        if target > current {
            // In test mode, we don't actually sleep, just advance time
            let mut time = self.current_time.lock().await;
            *time = target;
        }
    }
}

impl Default for MockDiscordApi {
    fn default() -> Self {
        Self::new()
    }
}
