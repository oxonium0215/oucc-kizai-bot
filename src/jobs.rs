use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, error, warn};

use crate::models::Job;
use crate::performance::{MessageUpdateManager, MetricsCollector};

pub struct JobWorker {
    db: SqlitePool,
    metrics_collector: MetricsCollector,
}

impl JobWorker {
    pub fn new(db: SqlitePool) -> Self {
        let metrics_collector = MetricsCollector::new(db.clone());
        Self { db, metrics_collector }
    }

    pub async fn run(&self) -> Result<()> {
        info!("Starting job worker");
        
        loop {
            if let Err(e) = self.process_jobs().await {
                error!("Error processing jobs: {}", e);
            }
            
            sleep(Duration::from_secs(30)).await;
        }
    }

    async fn process_jobs(&self) -> Result<()> {
        let now = Utc::now();
        
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

    async fn process_job(&self, job: &Job) -> Result<()> {
        info!("Processing job {} of type {}", job.id, job.job_type);
        
        // Mark job as running
        sqlx::query(
            "UPDATE jobs SET status = 'Running', updated_at = CURRENT_TIMESTAMP WHERE id = ?"
        )
        .bind(job.id)
        .execute(&self.db)
        .await?;

        // Process based on job type
        match job.job_type.as_str() {
            "reminder" => self.process_reminder(job).await?,
            "transfer_timeout" => self.process_transfer_timeout(job).await?,
            "retry_dm" => self.process_retry_dm(job).await?,
            "batch_message_update" => self.process_batch_message_update(job).await?,
            "performance_metrics" => self.process_performance_metrics(job).await?,
            _ => {
                warn!("Unknown job type: {}", job.job_type);
            }
        }

        // Mark job as completed
        sqlx::query(
            "UPDATE jobs SET status = 'Completed', updated_at = CURRENT_TIMESTAMP WHERE id = ?"
        )
        .bind(job.id)
        .execute(&self.db)
        .await?;

        Ok(())
    }

    async fn process_reminder(&self, _job: &Job) -> Result<()> {
        // TODO: Implement reminder processing
        info!("Processing reminder job");
        Ok(())
    }

    async fn process_transfer_timeout(&self, _job: &Job) -> Result<()> {
        // TODO: Implement transfer timeout processing
        info!("Processing transfer timeout job");
        Ok(())
    }

    async fn process_retry_dm(&self, _job: &Job) -> Result<()> {
        // TODO: Implement DM retry processing
        info!("Processing DM retry job");
        Ok(())
    }

    async fn process_batch_message_update(&self, job: &Job) -> Result<()> {
        info!("Processing batch message update job");
        
        // Parse the job payload to get message update details
        let payload: serde_json::Value = serde_json::from_str(&job.payload)?;
        
        // For now, just log the job - in a full implementation, this would
        // coordinate with the MessageUpdateManager
        info!("Batch message update payload: {}", payload);
        
        Ok(())
    }

    async fn process_performance_metrics(&self, _job: &Job) -> Result<()> {
        info!("Processing performance metrics collection job");
        
        // Store current metrics to database
        self.metrics_collector.store_metrics().await?;
        
        info!("Performance metrics stored successfully");
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
            let retry_delay = Duration::from_secs(300); // 5 minutes
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

    /// Schedule a performance metrics collection job
    pub async fn schedule_metrics_collection(&self) -> Result<()> {
        let scheduled_time = Utc::now() + chrono::Duration::minutes(5);
        
        sqlx::query(
            "INSERT INTO jobs (job_type, payload, scheduled_for, max_attempts) VALUES (?, ?, ?, ?)"
        )
        .bind("performance_metrics")
        .bind("{}")
        .bind(scheduled_time)
        .bind(3)
        .execute(&self.db)
        .await?;

        info!("Scheduled performance metrics collection job");
        Ok(())
    }

    /// Schedule batch message updates with priority and batching
    pub async fn schedule_batch_message_update(&self, guild_id: i64, priority: &str) -> Result<()> {
        let payload = serde_json::json!({
            "guild_id": guild_id,
            "priority": priority,
            "batch_size": 10
        });
        
        let scheduled_time = Utc::now() + chrono::Duration::seconds(5);
        
        sqlx::query(
            "INSERT INTO jobs (job_type, payload, scheduled_for, max_attempts) VALUES (?, ?, ?, ?)"
        )
        .bind("batch_message_update")
        .bind(payload.to_string())
        .bind(scheduled_time)
        .bind(3)
        .execute(&self.db)
        .await?;

        info!("Scheduled batch message update job for guild {}", guild_id);
        Ok(())
    }
}