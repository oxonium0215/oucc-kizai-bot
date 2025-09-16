use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use oucc_kizai_bot::{models::*, traits::*};
use serenity::model::prelude::*;
use std::sync::Arc;

mod common;

/// Simulate job worker processing with test dependencies
pub struct TestJobWorker {
    db: sqlx::SqlitePool,
    discord_api: Arc<dyn DiscordApi>,
    clock: Arc<dyn Clock>,
}

impl TestJobWorker {
    pub fn new(
        db: sqlx::SqlitePool,
        discord_api: Arc<dyn DiscordApi>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            db,
            discord_api,
            clock,
        }
    }
    
    /// Process all pending jobs
    pub async fn process_pending_jobs(&self) -> Result<usize> {
        let current_time = self.clock.now_utc();
        
        let jobs = sqlx::query_as!(
            Job,
            "SELECT * FROM jobs 
             WHERE status = 'Pending' AND scheduled_for <= ?
             ORDER BY scheduled_for ASC",
            current_time
        )
        .fetch_all(&self.db)
        .await?;
        
        let mut processed = 0;
        for job in jobs {
            if let Err(e) = self.process_job(&job).await {
                eprintln!("Failed to process job {}: {}", job.id, e);
                self.mark_job_failed(&job).await?;
            } else {
                processed += 1;
            }
        }
        
        Ok(processed)
    }
    
    async fn process_job(&self, job: &Job) -> Result<()> {
        // Mark job as running
        sqlx::query!(
            "UPDATE jobs SET status = 'Running', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            job.id
        )
        .execute(&self.db)
        .await?;
        
        // Process based on job type
        match job.job_type.as_str() {
            "reminder" => self.process_reminder(job).await?,
            "transfer_timeout" => self.process_transfer_timeout(job).await?,
            "retry_dm" => self.process_retry_dm(job).await?,
            _ => {
                return Err(anyhow::anyhow!("Unknown job type: {}", job.job_type));
            }
        }
        
        // Mark job as completed
        sqlx::query!(
            "UPDATE jobs SET status = 'Completed', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            job.id
        )
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
    
    async fn process_reminder(&self, job: &Job) -> Result<()> {
        let payload: serde_json::Value = serde_json::from_str(&job.payload)?;
        let reservation_id = payload["reservation_id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing reservation_id in reminder payload"))?;
        let reminder_type = payload["type"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing type in reminder payload"))?;
        
        // Get reservation details
        let reservation = sqlx::query_as!(
            Reservation,
            "SELECT * FROM reservations WHERE id = ?",
            reservation_id
        )
        .fetch_one(&self.db)
        .await?;
        
        let equipment = sqlx::query_as!(
            Equipment,
            "SELECT * FROM equipment WHERE id = ?",
            reservation.equipment_id
        )
        .fetch_one(&self.db)
        .await?;
        
        let user_id = UserId::new(reservation.user_id as u64);
        
        let message = match reminder_type {
            "pre_end" => format!(
                "📅 リマインダー: 「{}」の貸出期限まで15分です。\n返却時刻: {}",
                equipment.name,
                oucc_kizai_bot::time::utc_to_jst_string(reservation.end_time)
            ),
            "return_delay" => format!(
                "⚠️ 返却遅延: 「{}」の返却期限が過ぎています。\n期限: {}",
                equipment.name,
                oucc_kizai_bot::time::utc_to_jst_string(reservation.end_time)
            ),
            _ => return Err(anyhow::anyhow!("Unknown reminder type: {}", reminder_type)),
        };
        
        // Try to send DM, fallback to mention if DM fails
        match self.discord_api.send_dm(user_id, &message).await? {
            Some(_) => {
                // DM sent successfully
            }
            None => {
                // DM failed, schedule mention fallback
                self.schedule_mention_fallback(user_id, &message).await?;
            }
        }
        
        Ok(())
    }
    
    async fn process_transfer_timeout(&self, job: &Job) -> Result<()> {
        let payload: serde_json::Value = serde_json::from_str(&job.payload)?;
        let transfer_id = payload["transfer_id"]
            .as_i64()
            .ok_or_else(|| anyhow::anyhow!("Missing transfer_id in transfer_timeout payload"))?;
        
        // Get transfer request
        let transfer = sqlx::query_as!(
            TransferRequest,
            "SELECT * FROM transfer_requests WHERE id = ? AND status = 'Pending'",
            transfer_id
        )
        .fetch_optional(&self.db)
        .await?;
        
        if let Some(transfer) = transfer {
            // Mark as expired
            sqlx::query!(
                "UPDATE transfer_requests SET status = 'Expired', updated_at = CURRENT_TIMESTAMP WHERE id = ?",
                transfer.id
            )
            .execute(&self.db)
            .await?;
            
            // Notify requester
            let requester_id = UserId::new(transfer.from_user_id as u64);
            let message = "⏰ 機材の移譲リクエストが期限切れになりました。";
            
            match self.discord_api.send_dm(requester_id, message).await? {
                Some(_) => {}
                None => {
                    self.schedule_mention_fallback(requester_id, message).await?;
                }
            }
        }
        
        Ok(())
    }
    
    async fn process_retry_dm(&self, job: &Job) -> Result<()> {
        let payload: serde_json::Value = serde_json::from_str(&job.payload)?;
        let user_id = payload["user_id"]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("Missing user_id in retry_dm payload"))?;
        let message = payload["message"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing message in retry_dm payload"))?;
        
        let user_id = UserId::new(user_id);
        
        match self.discord_api.send_dm(user_id, message).await? {
            Some(_) => {
                // Success, no further action needed
            }
            None => {
                // Still failing, schedule mention fallback
                self.schedule_mention_fallback(user_id, message).await?;
            }
        }
        
        Ok(())
    }
    
    async fn schedule_mention_fallback(&self, user_id: UserId, message: &str) -> Result<()> {
        // In a real implementation, this would post to the reservation channel
        // For testing, we'll send to a mock channel
        let channel_id = ChannelId::new(123456789);
        let mention_message = format!("<@{}> {}", user_id.get(), message);
        
        self.discord_api.send_channel_message(channel_id, &mention_message).await?;
        Ok(())
    }
    
    async fn mark_job_failed(&self, job: &Job) -> Result<()> {
        sqlx::query!(
            "UPDATE jobs SET status = 'Failed', attempts = attempts + 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
            job.id
        )
        .execute(&self.db)
        .await?;
        Ok(())
    }
}

/// Schedule a job in the database
pub async fn schedule_job(
    db: &sqlx::SqlitePool,
    job_type: &str,
    payload: serde_json::Value,
    scheduled_for: DateTime<Utc>,
) -> Result<Job> {
    let now = Utc::now();
    let payload_str = serde_json::to_string(&payload)?;
    
    let result = sqlx::query!(
        "INSERT INTO jobs (job_type, payload, scheduled_for, status, created_at, updated_at)
         VALUES (?, ?, ?, 'Pending', ?, ?) RETURNING id",
        job_type,
        payload_str,
        scheduled_for,
        now,
        now
    )
    .fetch_one(db)
    .await?;
    
    Ok(Job {
        id: result.id,
        job_type: job_type.to_string(),
        payload: payload_str,
        scheduled_for,
        status: "Pending".to_string(),
        attempts: 0,
        max_attempts: 3,
        created_at: now,
        updated_at: now,
    })
}

#[tokio::test]
async fn test_transfer_timeout_job() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    // Create a reservation and transfer request
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        12345,
        ctx.clock.now_utc(),
        ctx.clock.now_utc() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;
    
    let now = ctx.clock.now_utc();
    let expires_at = now + Duration::hours(3);
    
    let transfer_result = sqlx::query!(
        "INSERT INTO transfer_requests (reservation_id, from_user_id, to_user_id, expires_at, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, 'Pending', ?, ?) RETURNING id",
        reservation.id,
        12345,
        67890,
        expires_at,
        now,
        now
    )
    .fetch_one(&ctx.db)
    .await?;
    
    // Schedule transfer timeout job
    let job_payload = serde_json::json!({
        "transfer_id": transfer_result.id
    });
    
    schedule_job(
        &ctx.db,
        "transfer_timeout",
        job_payload,
        expires_at,
    ).await?;
    
    // Advance clock past expiry time
    ctx.clock.advance(Duration::hours(4)).await;
    
    // Process jobs
    let worker = TestJobWorker::new(
        ctx.db.clone(),
        ctx.discord_api.clone(),
        ctx.clock.clone(),
    );
    
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 1);
    
    // Verify transfer was marked as expired
    let transfer = sqlx::query!(
        "SELECT status FROM transfer_requests WHERE id = ?",
        transfer_result.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(transfer.status, "Expired");
    
    // Verify notification was sent
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert_eq!(sent_dms[0].0, UserId::new(12345));
    assert!(sent_dms[0].1.contains("期限切れ"));
    
    Ok(())
}

#[tokio::test]
async fn test_reminder_jobs() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    let start_time = ctx.clock.now_utc();
    let end_time = start_time + Duration::hours(2);
    
    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        12345,
        start_time,
        end_time,
    )
    .build(&ctx.db)
    .await?;
    
    // Schedule pre-end reminder (15 minutes before end)
    let pre_end_time = end_time - Duration::minutes(15);
    let job_payload = serde_json::json!({
        "reservation_id": reservation.id,
        "type": "pre_end"
    });
    
    schedule_job(
        &ctx.db,
        "reminder",
        job_payload,
        pre_end_time,
    ).await?;
    
    // Schedule return delay reminder (after end time)
    let delay_time = end_time + Duration::minutes(30);
    let job_payload = serde_json::json!({
        "reservation_id": reservation.id,
        "type": "return_delay"
    });
    
    schedule_job(
        &ctx.db,
        "reminder",
        job_payload,
        delay_time,
    ).await?;
    
    let worker = TestJobWorker::new(
        ctx.db.clone(),
        ctx.discord_api.clone(),
        ctx.clock.clone(),
    );
    
    // Advance to pre-end reminder time
    ctx.clock.set_time(pre_end_time).await;
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 1);
    
    // Check pre-end reminder was sent
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert!(sent_dms[0].1.contains("15分"));
    assert!(sent_dms[0].1.contains(&equipment.name));
    
    // Clear and advance to delay reminder time
    ctx.discord_api.clear().await;
    ctx.clock.set_time(delay_time).await;
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 1);
    
    // Check delay reminder was sent
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert!(sent_dms[0].1.contains("遅延"));
    assert!(sent_dms[0].1.contains(&equipment.name));
    
    Ok(())
}

#[tokio::test]
async fn test_dm_failure_fallback() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    // Enable DM failure mode
    ctx.discord_api.set_dm_failure_mode(true).await;
    
    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        12345,
        ctx.clock.now_utc(),
        ctx.clock.now_utc() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;
    
    // Schedule a reminder
    let job_payload = serde_json::json!({
        "reservation_id": reservation.id,
        "type": "pre_end"
    });
    
    schedule_job(
        &ctx.db,
        "reminder",
        job_payload,
        ctx.clock.now_utc(),
    ).await?;
    
    let worker = TestJobWorker::new(
        ctx.db.clone(),
        ctx.discord_api.clone(),
        ctx.clock.clone(),
    );
    
    // Process job
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 1);
    
    // Verify DM was attempted but failed
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 0); // No DMs sent due to failure mode
    
    // Verify fallback channel message was sent
    let channel_messages = ctx.discord_api.get_channel_messages().await;
    assert_eq!(channel_messages.len(), 1);
    assert!(channel_messages[0].1.contains("<@12345>"));
    assert!(channel_messages[0].1.contains("15分"));
    
    Ok(())
}

#[tokio::test]
async fn test_retry_dm_job() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Enable DM failure mode initially
    ctx.discord_api.set_dm_failure_mode(true).await;
    
    // Schedule a retry DM job
    let job_payload = serde_json::json!({
        "user_id": 12345u64,
        "message": "Test retry message"
    });
    
    schedule_job(
        &ctx.db,
        "retry_dm",
        job_payload,
        ctx.clock.now_utc(),
    ).await?;
    
    let worker = TestJobWorker::new(
        ctx.db.clone(),
        ctx.discord_api.clone(),
        ctx.clock.clone(),
    );
    
    // Process job - should fail and fall back to mention
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 1);
    
    // Verify fallback was used
    let channel_messages = ctx.discord_api.get_channel_messages().await;
    assert_eq!(channel_messages.len(), 1);
    assert!(channel_messages[0].1.contains("<@12345>"));
    assert!(channel_messages[0].1.contains("Test retry message"));
    
    // Now disable DM failure mode and try again
    ctx.discord_api.clear().await;
    ctx.discord_api.set_dm_failure_mode(false).await;
    
    // Schedule another retry
    let job_payload = serde_json::json!({
        "user_id": 12345u64,
        "message": "Test successful retry"
    });
    
    schedule_job(
        &ctx.db,
        "retry_dm",
        job_payload,
        ctx.clock.now_utc(),
    ).await?;
    
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 1);
    
    // Verify DM was sent successfully
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert_eq!(sent_dms[0].1, "Test successful retry");
    
    // No fallback channel message should have been sent
    let channel_messages = ctx.discord_api.get_channel_messages().await;
    assert_eq!(channel_messages.len(), 0);
    
    Ok(())
}

#[tokio::test]
async fn test_job_failure_handling() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Schedule a job with invalid payload
    let job_payload = serde_json::json!({
        "invalid": "payload"
    });
    
    schedule_job(
        &ctx.db,
        "reminder",
        job_payload,
        ctx.clock.now_utc(),
    ).await?;
    
    let worker = TestJobWorker::new(
        ctx.db.clone(),
        ctx.discord_api.clone(),
        ctx.clock.clone(),
    );
    
    // Process job - should fail
    let processed = worker.process_pending_jobs().await?;
    assert_eq!(processed, 0); // Job failed, not counted as processed
    
    // Verify job was marked as failed
    let failed_jobs = sqlx::query!(
        "SELECT COUNT(*) as count FROM jobs WHERE status = 'Failed'"
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(failed_jobs.count, 1);
    
    Ok(())
}