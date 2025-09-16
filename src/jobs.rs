use anyhow::Result;
use chrono::Utc;
use sqlx::SqlitePool;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::models::Job;

pub struct JobWorker {
    db: SqlitePool,
}

impl JobWorker {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
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

    async fn process_reminder(&self, _job: &Job) -> Result<()> {
        // TODO: Implement reminder processing
        Ok(())
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
}
