use anyhow::Result;
use chrono::{DateTime, Duration, NaiveDateTime, Utc};
use serde_json::Value;
use serenity::model::prelude::*;
use sqlx::SqlitePool;
use std::time::Duration as StdDuration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::models::{DeliveryMethod, Job, ReminderKind};
use crate::time::utc_to_jst_string;
use crate::traits::DiscordApi;
use crate::transfer_notifications::{TransferNotificationService, TransferNotificationType};

// Helper function to convert NaiveDateTime to DateTime<Utc>
fn naive_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
    DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
}

pub struct JobWorker {
    db: SqlitePool,
    discord_api: Option<Box<dyn DiscordApi>>,
    notification_service: TransferNotificationService,
}

impl JobWorker {
    pub fn new(db: SqlitePool) -> Self {
        let notification_service = TransferNotificationService::new(db.clone());
        Self {
            db,
            discord_api: None,
            notification_service,
        }
    }

    pub fn with_discord_api(db: SqlitePool, discord_api: Box<dyn DiscordApi>) -> Self {
        let notification_service = TransferNotificationService::new(db.clone());
        Self {
            db,
            discord_api: Some(discord_api),
            notification_service,
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting job worker");

        loop {
            if let Err(e) = self.process_jobs().await {
                error!("Error processing jobs: {}", e);
            }

            sleep(StdDuration::from_secs(60)).await; // Changed to 60 seconds as per requirement
        }
    }

    async fn process_jobs(&self) -> Result<()> {
        let now = Utc::now();

        // Process scheduled transfers first
        self.process_scheduled_transfers().await?;
        
        // Process expired transfer requests
        self.process_expired_transfers().await?;

        // Get pending jobs that are due
        let jobs: Vec<Job> = sqlx::query_as::<_, Job>(
            "SELECT * FROM jobs WHERE status = 'Pending' AND scheduled_for <= ? ORDER BY scheduled_for LIMIT 10"
        )
        .bind(now)
        .fetch_all(&self.db)
        .await?;

        for job in jobs {
            if let Err(e) = self.process_job(&job).await {
                error!("Failed to process job {}: {}", job.id, e);
                self.mark_job_failed(&job).await?;
            }
        }

        Ok(())
    }

    /// Process scheduled transfers that are due for execution
    async fn process_scheduled_transfers(&self) -> Result<()> {
        let now = Utc::now().naive_utc();

        // Get pending transfer requests that are due for execution
        let transfer_rows = sqlx::query!(
            "SELECT * FROM transfer_requests 
             WHERE status = 'Pending' AND execute_at_utc IS NOT NULL AND execute_at_utc <= ?
             ORDER BY execute_at_utc LIMIT 10",
            now
        )
        .fetch_all(&self.db)
        .await?;

        // Convert to proper models
        let transfers: Vec<crate::models::TransferRequest> = transfer_rows
            .into_iter()
            .map(|row| crate::models::TransferRequest {
                id: row.id.unwrap_or(0),
                reservation_id: row.reservation_id,
                from_user_id: row.from_user_id,
                to_user_id: row.to_user_id,
                requested_by_user_id: row.requested_by_user_id.unwrap_or(row.from_user_id),
                execute_at_utc: row.execute_at_utc.map(naive_to_utc),
                note: row.note,
                expires_at: naive_to_utc(row.expires_at),
                status: row.status,
                canceled_at_utc: row.canceled_at_utc.map(naive_to_utc),
                canceled_by_user_id: row.canceled_by_user_id,
                created_at: naive_to_utc(row.created_at.unwrap_or_else(|| Utc::now().naive_utc())),
                updated_at: naive_to_utc(row.updated_at.unwrap_or_else(|| Utc::now().naive_utc())),
            })
            .collect();

        for transfer in transfers {
            if let Err(e) = self.execute_scheduled_transfer(&transfer).await {
                error!(
                    "Failed to execute scheduled transfer {}: {}",
                    transfer.id, e
                );
                // Mark transfer as failed but don't stop processing others
                let _ = self.mark_transfer_failed(&transfer).await;
            }
        }

        Ok(())
    }

    /// Execute a scheduled transfer
    async fn execute_scheduled_transfer(
        &self,
        transfer: &crate::models::TransferRequest,
    ) -> Result<()> {
        info!("Executing scheduled transfer {}", transfer.id);

        let mut tx = self.db.begin().await?;

        // Re-validate the transfer request and reservation
        let reservation = sqlx::query!(
            "SELECT r.id, r.equipment_id, r.user_id, r.start_time, r.end_time, r.status, r.returned_at,
                    e.name as equipment_name
             FROM reservations r
             JOIN equipment e ON r.equipment_id = e.id
             WHERE r.id = ? AND r.status = 'Confirmed'",
            transfer.reservation_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let reservation = match reservation {
            Some(res) => res,
            None => {
                // Reservation no longer exists or was cancelled
                warn!(
                    "Reservation {} for transfer {} no longer exists",
                    transfer.reservation_id, transfer.id
                );
                self.mark_transfer_expired(transfer).await?;
                return Ok(());
            }
        };

        // Check if reservation is already returned
        if reservation.returned_at.is_some() {
            warn!(
                "Reservation {} for transfer {} has already been returned",
                transfer.reservation_id, transfer.id
            );
            self.mark_transfer_expired(transfer).await?;
            return Ok(());
        }

        // Check if reservation has ended
        let now = chrono::Utc::now();
        let reservation_end_time = naive_to_utc(reservation.end_time);
        if reservation_end_time <= now {
            warn!(
                "Reservation {} for transfer {} has already ended",
                transfer.reservation_id, transfer.id
            );
            self.mark_transfer_expired(transfer).await?;
            return Ok(());
        }

        // Execute the transfer
        // Update reservation owner
        sqlx::query!(
            "UPDATE reservations SET user_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            transfer.to_user_id,
            transfer.reservation_id
        )
        .execute(&mut *tx)
        .await?;

        // Log the transfer
        let log_note = format!(
            "Scheduled transfer executed: from <@{}> to <@{}> by <@{}> - Reservation ID: {}{}",
            transfer.from_user_id,
            transfer.to_user_id,
            transfer.requested_by_user_id,
            transfer.reservation_id,
            if let Some(note) = &transfer.note {
                format!(" - Note: {}", note)
            } else {
                String::new()
            }
        );

        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, 'Transferred', NULL, 'Confirmed', 'Confirmed', ?, CURRENT_TIMESTAMP)",
            reservation.equipment_id,
            transfer.requested_by_user_id,
            log_note
        )
        .execute(&mut *tx)
        .await?;

        // Mark transfer as completed
        sqlx::query!(
            "UPDATE transfer_requests SET status = 'Accepted', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            transfer.id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        info!("Successfully executed scheduled transfer {}", transfer.id);

        // TODO: Send DM notifications to old and new owners (best-effort)
        // This would use the notification infrastructure

        Ok(())
    }

    /// Mark a transfer as failed due to execution errors
    async fn mark_transfer_failed(&self, transfer: &crate::models::TransferRequest) -> Result<()> {
        sqlx::query!(
            "UPDATE transfer_requests SET status = 'Expired', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            transfer.id
        )
        .execute(&self.db)
        .await?;

        warn!(
            "Marked transfer {} as failed/expired due to execution error",
            transfer.id
        );
        Ok(())
    }

    /// Mark a transfer as expired due to invalid conditions
    async fn mark_transfer_expired(&self, transfer: &crate::models::TransferRequest) -> Result<()> {
        sqlx::query!(
            "UPDATE transfer_requests SET status = 'Expired', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            transfer.id
        )
        .execute(&self.db)
        .await?;

        info!("Marked transfer {} as expired", transfer.id);
        Ok(())
    }

    /// Process transfer requests that have expired (3 hours with no response)
    async fn process_expired_transfers(&self) -> Result<()> {
        let now = Utc::now().naive_utc();

        // Get pending transfer requests that have expired
        let expired_transfer_rows = sqlx::query!(
            "SELECT * FROM transfer_requests 
             WHERE status = 'Pending' AND execute_at_utc IS NULL AND expires_at <= ?
             ORDER BY expires_at LIMIT 10",
            now
        )
        .fetch_all(&self.db)
        .await?;

        info!("Found {} expired transfer requests", expired_transfer_rows.len());

        for row in expired_transfer_rows {
            // Convert to model
            let transfer = crate::models::TransferRequest {
                id: row.id.unwrap_or(0),
                reservation_id: row.reservation_id,
                from_user_id: row.from_user_id,
                to_user_id: row.to_user_id,
                requested_by_user_id: row.requested_by_user_id.unwrap_or(row.from_user_id),
                execute_at_utc: row.execute_at_utc.map(naive_to_utc),
                note: row.note,
                expires_at: naive_to_utc(row.expires_at),
                status: row.status,
                canceled_at_utc: row.canceled_at_utc.map(naive_to_utc),
                canceled_by_user_id: row.canceled_by_user_id,
                created_at: naive_to_utc(row.created_at.unwrap_or(chrono::Utc::now().naive_utc())),
                updated_at: naive_to_utc(row.updated_at.unwrap_or(chrono::Utc::now().naive_utc())),
            };

            if let Err(e) = self.handle_transfer_timeout(&transfer).await {
                error!("Failed to handle timeout for transfer {}: {}", transfer.id, e);
            }
        }

        Ok(())
    }

    /// Handle a transfer request that has timed out
    async fn handle_transfer_timeout(&self, transfer: &crate::models::TransferRequest) -> Result<()> {
        info!("Handling timeout for transfer {}", transfer.id);

        // Mark transfer as expired
        sqlx::query!(
            "UPDATE transfer_requests SET status = 'Expired', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            transfer.id
        )
        .execute(&self.db)
        .await?;

        // Get equipment details for notification
        let equipment_details = sqlx::query!(
            "SELECT e.id as equipment_id, e.guild_id, e.name as equipment_name
             FROM transfer_requests tr
             JOIN reservations r ON tr.reservation_id = r.id
             JOIN equipment e ON r.equipment_id = e.id
             WHERE tr.id = ?",
            transfer.id
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(details) = equipment_details {
            // Send timeout notification using the notification service
            let notification = TransferNotificationType::Expired {
                equipment_name: details.equipment_name.clone(),
            };

            if let Some(discord_api) = &self.discord_api {
                if let Err(e) = self.notification_service.send_notification_with_api(
                    discord_api.as_ref(),
                    transfer.requested_by_user_id,
                    transfer.reservation_id,
                    details.equipment_id,
                    details.guild_id,
                    notification,
                ).await {
                    error!("Failed to send transfer timeout notification: {}", e);
                }
            } else {
                warn!("No Discord API available for transfer timeout notification");
            }
        }

        info!("Successfully handled timeout for transfer {}", transfer.id);
        Ok(())
    }

    async fn process_job(&self, job: &Job) -> Result<()> {
        info!("Processing job {} of type {}", job.id, job.job_type);

        // Mark job as running
        sqlx::query(
            "UPDATE jobs SET status = 'Running', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(job.id)
        .execute(&self.db)
        .await?;

        // Process based on job type
        match job.job_type.as_str() {
            "reminder" => self.process_reminder(job).await?,
            "transfer_timeout" => self.process_transfer_timeout(job).await?,
            "retry_dm" => self.process_retry_dm(job).await?,
            _ => {
                warn!("Unknown job type: {}", job.job_type);
            }
        }

        // Mark job as completed
        sqlx::query(
            "UPDATE jobs SET status = 'Completed', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        )
        .bind(job.id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    async fn process_reminder(&self, job: &Job) -> Result<()> {
        let payload: Value = serde_json::from_str(&job.payload)?;
        let reservation_id = payload["reservation_id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing reservation_id in job payload"))?;
        let reminder_type = payload["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing type in job payload"))?;

        // Parse reminder kind
        let reminder_kind = match reminder_type {
            "pre_start" => ReminderKind::PreStart,
            "start" => ReminderKind::Start,
            "pre_end" => ReminderKind::PreEnd,
            s if s.starts_with("return_delay") => {
                // Extract overdue count from type like "return_delay_1", "return_delay_2"
                let overdue_num = if s == "return_delay" {
                    1
                } else {
                    s.strip_prefix("return_delay_")
                        .and_then(|n| n.parse::<u32>().ok())
                        .unwrap_or(1)
                };
                ReminderKind::Overdue(overdue_num)
            }
            _ => return Err(anyhow::anyhow!("Unknown reminder type: {}", reminder_type)),
        };

        // Check if reminder already sent (idempotency)
        let reminder_kind_str = reminder_kind.to_db_string();
        let existing_reminder = sqlx::query!(
            "SELECT id FROM sent_reminders WHERE reservation_id = ? AND kind = ?",
            reservation_id,
            reminder_kind_str
        )
        .fetch_optional(&self.db)
        .await?;

        if existing_reminder.is_some() {
            info!(
                "Reminder already sent for reservation {} kind {}",
                reservation_id,
                reminder_kind.to_db_string()
            );
            return Ok(());
        }

        // Get reservation details
        let reservation_row = sqlx::query!(
            "SELECT * FROM reservations WHERE id = ? AND status = 'Confirmed'",
            reservation_id
        )
        .fetch_optional(&self.db)
        .await?;

        let reservation_row = match reservation_row {
            Some(r) => r,
            None => {
                info!(
                    "Reservation {} not found or not confirmed, skipping reminder",
                    reservation_id
                );
                return Ok(());
            }
        };

        // Skip if already returned
        if reservation_row.returned_at.is_some() {
            info!(
                "Reservation {} already returned, skipping reminder",
                reservation_id
            );
            return Ok(());
        }

        // Get equipment details
        let equipment_row = sqlx::query!(
            "SELECT * FROM equipment WHERE id = ?",
            reservation_row.equipment_id
        )
        .fetch_one(&self.db)
        .await?;

        // Get guild configuration for fallback behavior
        let guild_row = sqlx::query!("SELECT * FROM guilds WHERE id = ?", equipment_row.guild_id)
            .fetch_one(&self.db)
            .await?;

        // Format reminder message - get individual fields safely
        let equipment_name: String = equipment_row.name;
        let start_time_naive: chrono::NaiveDateTime = reservation_row.start_time;
        let end_time_naive: chrono::NaiveDateTime = reservation_row.end_time;

        // Convert to UTC DateTime
        let start_time_utc = DateTime::<Utc>::from_naive_utc_and_offset(start_time_naive, Utc);
        let end_time_utc = DateTime::<Utc>::from_naive_utc_and_offset(end_time_naive, Utc);

        let message = match &reminder_kind {
            ReminderKind::PreStart => format!(
                "📅 リマインダー: 「{}」の貸出開始まで15分です。\n開始時刻: {}",
                equipment_name,
                utc_to_jst_string(start_time_utc)
            ),
            ReminderKind::Start => format!(
                "📅 貸出開始: 「{}」の貸出が開始されました。\n貸出時刻: {}",
                equipment_name,
                utc_to_jst_string(start_time_utc)
            ),
            ReminderKind::PreEnd => format!(
                "📅 リマインダー: 「{}」の貸出期限まで15分です。\n返却時刻: {}",
                equipment_name,
                utc_to_jst_string(end_time_utc)
            ),
            ReminderKind::Overdue(count) => format!(
                "⚠️ 返却遅延 #{}: 「{}」の返却期限が過ぎています。\n期限: {}",
                count,
                equipment_name,
                utc_to_jst_string(end_time_utc)
            ),
        };

        // Try sending reminder
        let delivery_method = if let Some(discord_api) = &self.discord_api {
            self.send_reminder_with_fallback(
                discord_api.as_ref(),
                reservation_row.user_id,
                &message,
                guild_row.reservation_channel_id,
                guild_row.dm_fallback_channel_enabled.unwrap_or(true),
            )
            .await?
        } else {
            DeliveryMethod::Failed
        };

        // Record that we sent this reminder
        let reminder_kind_str = reminder_kind.to_db_string();
        let now = Utc::now();
        let delivery_method_str = String::from(delivery_method);

        sqlx::query!(
            "INSERT INTO sent_reminders (reservation_id, kind, sent_at_utc, delivery_method)
             VALUES (?, ?, ?, ?)",
            reservation_id,
            reminder_kind_str,
            now,
            delivery_method_str
        )
        .execute(&self.db)
        .await?;

        info!(
            "Sent {} reminder for reservation {} via {:?}",
            reminder_kind.to_db_string(),
            reservation_id,
            delivery_method
        );

        Ok(())
    }

    async fn send_reminder_with_fallback(
        &self,
        discord_api: &dyn DiscordApi,
        user_id: i64,
        message: &str,
        reservation_channel_id: Option<i64>,
        dm_fallback_enabled: bool,
    ) -> Result<DeliveryMethod> {
        let user_id = UserId::new(user_id as u64);

        // Try sending DM first
        match discord_api.send_dm(user_id, message).await {
            Ok(Some(_)) => return Ok(DeliveryMethod::Dm),
            Ok(None) => {
                // DM failed (user has DMs disabled)
                info!("DM failed for user {}, attempting fallback", user_id);
            }
            Err(e) => {
                warn!("Error sending DM to user {}: {}", user_id, e);
            }
        }

        // Fallback to channel mention if enabled and channel is configured
        if dm_fallback_enabled {
            if let Some(channel_id) = reservation_channel_id {
                let channel_message = format!("<@{}> {}", user_id, message);
                let channel_id = ChannelId::new(channel_id as u64);

                match discord_api
                    .send_channel_message(channel_id, &channel_message)
                    .await
                {
                    Ok(_) => return Ok(DeliveryMethod::Channel),
                    Err(e) => {
                        warn!("Error sending channel fallback message: {}", e);
                    }
                }
            }
        }

        Ok(DeliveryMethod::Failed)
    }

    async fn process_transfer_timeout(&self, _job: &Job) -> Result<()> {
        // TODO: Implement transfer timeout processing
        Ok(())
    }

    async fn process_retry_dm(&self, _job: &Job) -> Result<()> {
        // TODO: Implement DM retry processing
        Ok(())
    }

    async fn mark_job_failed(&self, job: &Job) -> Result<()> {
        let new_attempts = job.attempts + 1;

        if new_attempts >= job.max_attempts {
            sqlx::query(
                "UPDATE jobs SET status = 'Failed', attempts = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?"
            )
            .bind(new_attempts)
            .bind(job.id)
            .execute(&self.db)
            .await?;
        } else {
            // Reschedule for retry
            let retry_delay = StdDuration::from_secs(300); // 5 minutes
            let new_scheduled_for = Utc::now() + chrono::Duration::from_std(retry_delay)?;

            sqlx::query(
                "UPDATE jobs SET status = 'Pending', attempts = ?, scheduled_for = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?"
            )
            .bind(new_attempts)
            .bind(new_scheduled_for)
            .bind(job.id)
            .execute(&self.db)
            .await?;
        }

        Ok(())
    }
}

/// Utility functions for scheduling reminder jobs
impl JobWorker {
    /// Schedule all reminder jobs for a reservation
    pub async fn schedule_reservation_reminders(
        db: &SqlitePool,
        reservation_id: i64,
        reservation_start: DateTime<Utc>,
        reservation_end: DateTime<Utc>,
        guild_id: i64,
    ) -> Result<()> {
        // Get guild notification preferences
        let guild_row = sqlx::query!("SELECT * FROM guilds WHERE id = ?", guild_id)
            .fetch_one(db)
            .await?;

        let pre_start_minutes = guild_row.pre_start_minutes.unwrap_or(15);
        let pre_end_minutes = guild_row.pre_end_minutes.unwrap_or(15);

        // Schedule pre-start reminder
        if pre_start_minutes > 0 {
            let pre_start_time = reservation_start - Duration::minutes(pre_start_minutes);
            if pre_start_time > Utc::now() {
                Self::schedule_reminder_job(db, reservation_id, "pre_start", pre_start_time)
                    .await?;
            }
        }

        // Schedule start reminder
        if reservation_start > Utc::now() {
            Self::schedule_reminder_job(db, reservation_id, "start", reservation_start).await?;
        }

        // Schedule pre-end reminder
        if pre_end_minutes > 0 {
            let pre_end_time = reservation_end - Duration::minutes(pre_end_minutes);
            if pre_end_time > Utc::now() {
                Self::schedule_reminder_job(db, reservation_id, "pre_end", pre_end_time).await?;
            }
        }

        Ok(())
    }

    /// Schedule overdue reminders for a reservation
    pub async fn schedule_overdue_reminders(
        db: &SqlitePool,
        reservation_id: i64,
        reservation_end: DateTime<Utc>,
        guild_id: i64,
    ) -> Result<()> {
        let guild_row = sqlx::query!("SELECT * FROM guilds WHERE id = ?", guild_id)
            .fetch_one(db)
            .await?;

        let repeat_hours = guild_row.overdue_repeat_hours.unwrap_or(12);
        let max_count = guild_row.overdue_max_count.unwrap_or(3);

        for i in 1..=max_count {
            let overdue_time = reservation_end + Duration::hours(repeat_hours * i);
            if overdue_time > Utc::now() {
                let job_type = if i == 1 {
                    "return_delay".to_string()
                } else {
                    format!("return_delay_{}", i)
                };

                Self::schedule_reminder_job(db, reservation_id, &job_type, overdue_time).await?;
            }
        }

        Ok(())
    }

    async fn schedule_reminder_job(
        db: &SqlitePool,
        reservation_id: i64,
        reminder_type: &str,
        scheduled_for: DateTime<Utc>,
    ) -> Result<()> {
        let payload = serde_json::json!({
            "reservation_id": reservation_id,
            "type": reminder_type
        });
        let payload_str = payload.to_string();

        sqlx::query!(
            "INSERT INTO jobs (job_type, payload, scheduled_for)
             VALUES ('reminder', ?, ?)",
            payload_str,
            scheduled_for
        )
        .execute(db)
        .await?;

        info!(
            "Scheduled {} reminder for reservation {} at {}",
            reminder_type, reservation_id, scheduled_for
        );

        Ok(())
    }

    /// Cancel all future reminders for a reservation (when returned)
    pub async fn cancel_reservation_reminders(db: &SqlitePool, reservation_id: i64) -> Result<()> {
        // Cancel pending reminder jobs
        sqlx::query!(
            "UPDATE jobs 
             SET status = 'Cancelled', updated_at = CURRENT_TIMESTAMP
             WHERE job_type = 'reminder' 
             AND status = 'Pending'
             AND JSON_EXTRACT(payload, '$.reservation_id') = ?",
            reservation_id
        )
        .execute(db)
        .await?;

        info!(
            "Cancelled future reminders for reservation {}",
            reservation_id
        );
        Ok(())
    }
}
