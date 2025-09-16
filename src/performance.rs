use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serenity::prelude::*;
use serenity::model::prelude::*;
use sqlx::SqlitePool;
use std::collections::{VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Configuration for Discord API rate limiting and batching
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum messages to update per batch
    pub batch_size: usize,
    /// Delay between batches in milliseconds
    pub batch_delay_ms: u64,
    /// Maximum concurrent message update operations
    pub max_concurrent_updates: usize,
    /// Base delay for exponential backoff in milliseconds
    pub base_backoff_ms: u64,
    /// Maximum backoff delay in milliseconds
    pub max_backoff_ms: u64,
    /// Maximum number of retry attempts
    pub max_retries: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            batch_size: 10,
            batch_delay_ms: 1000,
            max_concurrent_updates: 3,
            base_backoff_ms: 1000,
            max_backoff_ms: 60000,
            max_retries: 5,
        }
    }
}

/// A queued message update operation
#[derive(Debug, Clone)]
pub struct MessageUpdate {
    pub guild_id: GuildId,
    pub channel_id: ChannelId,
    pub message_id: MessageId,
    pub embed: Option<serenity::all::CreateEmbed>,
    pub components: Vec<serenity::all::CreateActionRow>,
    pub priority: UpdatePriority,
    pub created_at: Instant,
    pub retries: usize,
}

/// Priority levels for message updates
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UpdatePriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
}

/// Batched message update manager
#[derive(Clone)]
pub struct MessageUpdateManager {
    config: RateLimitConfig,
    update_queue: Arc<Mutex<VecDeque<MessageUpdate>>>,
    rate_limiter: Arc<RwLock<RateLimiter>>,
    is_running: Arc<Mutex<bool>>,
}

/// Rate limiter with exponential backoff
#[derive(Debug)]
struct RateLimiter {
    last_request: Instant,
    current_backoff: Duration,
    consecutive_failures: usize,
    config: RateLimitConfig,
}

impl RateLimiter {
    fn new(config: RateLimitConfig) -> Self {
        Self {
            last_request: Instant::now(),
            current_backoff: Duration::from_millis(config.base_backoff_ms),
            consecutive_failures: 0,
            config,
        }
    }

    /// Calculate delay needed before next request
    async fn wait_for_rate_limit(&mut self) -> Duration {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_request);
        
        if elapsed < self.current_backoff {
            let wait_time = self.current_backoff - elapsed;
            debug!("Rate limiting: waiting {}ms", wait_time.as_millis());
            return wait_time;
        }
        
        Duration::ZERO
    }

    /// Record a successful request
    fn record_success(&mut self) {
        self.last_request = Instant::now();
        self.consecutive_failures = 0;
        self.current_backoff = Duration::from_millis(self.config.base_backoff_ms);
    }

    /// Record a failed request and increase backoff
    fn record_failure(&mut self) {
        self.last_request = Instant::now();
        self.consecutive_failures += 1;
        
        let new_backoff_ms = (self.config.base_backoff_ms as f64 * 2.0_f64.powi(self.consecutive_failures as i32)) as u64;
        let capped_backoff_ms = new_backoff_ms.min(self.config.max_backoff_ms);
        
        self.current_backoff = Duration::from_millis(capped_backoff_ms);
        warn!("Rate limit hit, backing off for {}ms (failure #{})", capped_backoff_ms, self.consecutive_failures);
    }
}

impl MessageUpdateManager {
    pub fn new(config: Option<RateLimitConfig>) -> Self {
        let config = config.unwrap_or_default();
        
        Self {
            rate_limiter: Arc::new(RwLock::new(RateLimiter::new(config.clone()))),
            config,
            update_queue: Arc::new(Mutex::new(VecDeque::new())),
            is_running: Arc::new(Mutex::new(false)),
        }
    }

    /// Queue a message update operation
    pub async fn queue_update(&self, update: MessageUpdate) {
        let mut queue = self.update_queue.lock().await;
        
        // Insert based on priority (higher priority first)
        let insert_pos = queue
            .iter()
            .position(|existing| existing.priority < update.priority)
            .unwrap_or(queue.len());
            
        queue.insert(insert_pos, update);
        
        debug!("Queued message update, queue size: {}", queue.len());
    }

    /// Start the background message update processor
    pub async fn start_processor(&self, ctx: Arc<Context>) -> Result<()> {
        let mut is_running = self.is_running.lock().await;
        if *is_running {
            return Ok(());
        }
        *is_running = true;
        drop(is_running);

        let queue = self.update_queue.clone();
        let rate_limiter = self.rate_limiter.clone();
        let config = self.config.clone();
        let is_running = self.is_running.clone();

        tokio::spawn(async move {
            info!("Started message update processor");
            
            while *is_running.lock().await {
                if let Err(e) = Self::process_batch(
                    &queue,
                    &rate_limiter,
                    &config,
                    &ctx,
                ).await {
                    error!("Error processing message update batch: {}", e);
                }
                
                sleep(Duration::from_millis(config.batch_delay_ms)).await;
            }
            
            info!("Stopped message update processor");
        });

        Ok(())
    }

    /// Stop the background processor
    pub async fn stop_processor(&self) {
        let mut is_running = self.is_running.lock().await;
        *is_running = false;
    }

    /// Process a batch of message updates
    async fn process_batch(
        queue: &Arc<Mutex<VecDeque<MessageUpdate>>>,
        rate_limiter: &Arc<RwLock<RateLimiter>>,
        config: &RateLimitConfig,
        ctx: &Context,
    ) -> Result<()> {
        // Get batch of updates
        let batch = {
            let mut queue = queue.lock().await;
            let batch_size = config.batch_size.min(queue.len());
            
            if batch_size == 0 {
                return Ok(());
            }
            
            queue.drain(0..batch_size).collect::<Vec<_>>()
        };

        debug!("Processing batch of {} message updates", batch.len());

        // Apply rate limiting
        {
            let mut limiter = rate_limiter.write().await;
            let wait_time = limiter.wait_for_rate_limit().await;
            if wait_time > Duration::ZERO {
                drop(limiter);
                sleep(wait_time).await;
            }
        }

        // Process updates with limited concurrency
        let semaphore = Arc::new(tokio::sync::Semaphore::new(config.max_concurrent_updates));
        let mut handles = Vec::new();

        for update in batch {
            let permit = semaphore.clone().acquire_owned().await?;
            let ctx = ctx.clone();
            let rate_limiter = rate_limiter.clone();
            
            let handle = tokio::spawn(async move {
                let _permit = permit;
                Self::process_single_update(update, &ctx, &rate_limiter).await
            });
            
            handles.push(handle);
        }

        // Wait for all updates to complete
        let mut successful = 0;
        let mut failed_updates = Vec::new();
        
        for handle in handles {
            match handle.await? {
                Ok(()) => successful += 1,
                Err((update, error)) => {
                    error!("Failed to update message: {}", error);
                    failed_updates.push(update);
                }
            }
        }

        // Requeue failed updates with retry logic
        if !failed_updates.is_empty() {
            let failed_count = failed_updates.len();
            let mut queue = queue.lock().await;
            for mut update in failed_updates {
                update.retries += 1;
                if update.retries < config.max_retries {
                    queue.push_back(update);
                } else {
                    error!("Dropping message update after {} retries", config.max_retries);
                }
            }
            debug!("Processed batch: {} successful, {} failed", successful, failed_count);
        } else {
            debug!("Processed batch: {} successful, {} failed", successful, 0);
        }
        Ok(())
    }

    /// Process a single message update
    async fn process_single_update(
        update: MessageUpdate,
        ctx: &Context,
        rate_limiter: &Arc<RwLock<RateLimiter>>,
    ) -> Result<(), (MessageUpdate, anyhow::Error)> {
        let start_time = Instant::now();
        
        let result = async {
            let mut edit_message = serenity::all::EditMessage::new();
            
            if let Some(embed) = update.embed.as_ref() {
                edit_message = edit_message.embed(embed.clone());
            }
            
            if !update.components.is_empty() {
                edit_message = edit_message.components(update.components.clone());
            }

            update.channel_id
                .edit_message(&ctx.http, update.message_id, edit_message)
                .await
                .map_err(|e| anyhow::anyhow!("Discord API error: {}", e))
        }.await;

        match result {
            Ok(_) => {
                let elapsed = start_time.elapsed();
                debug!(
                    "Successfully updated message {} in {}ms",
                    update.message_id,
                    elapsed.as_millis()
                );
                
                // Record success for rate limiting
                rate_limiter.write().await.record_success();
                Ok(())
            }
            Err(e) => {
                // Check if this is a rate limit error
                if e.to_string().contains("429") || e.to_string().contains("rate") {
                    rate_limiter.write().await.record_failure();
                }
                
                Err((update, e))
            }
        }
    }

    /// Get current queue statistics
    pub async fn get_stats(&self) -> QueueStats {
        let queue = self.update_queue.lock().await;
        let rate_limiter = self.rate_limiter.read().await;
        
        QueueStats {
            queue_size: queue.len(),
            current_backoff_ms: rate_limiter.current_backoff.as_millis() as u64,
            consecutive_failures: rate_limiter.consecutive_failures,
        }
    }
}

/// Statistics about the message update queue
#[derive(Debug, Serialize, Deserialize)]
pub struct QueueStats {
    pub queue_size: usize,
    pub current_backoff_ms: u64,
    pub consecutive_failures: usize,
}

/// Performance monitoring metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceMetrics {
    pub timestamp: DateTime<Utc>,
    pub messages_updated: u64,
    pub average_update_time_ms: f64,
    pub queue_size: usize,
    pub rate_limit_hits: u64,
    pub failed_updates: u64,
}

/// Performance metrics collector
pub struct MetricsCollector {
    db: SqlitePool,
    metrics: Arc<Mutex<PerformanceMetrics>>,
}

impl MetricsCollector {
    pub fn new(db: SqlitePool) -> Self {
        Self {
            db,
            metrics: Arc::new(Mutex::new(PerformanceMetrics {
                timestamp: Utc::now(),
                messages_updated: 0,
                average_update_time_ms: 0.0,
                queue_size: 0,
                rate_limit_hits: 0,
                failed_updates: 0,
            })),
        }
    }

    /// Record a successful message update
    pub async fn record_update(&self, duration: Duration) {
        let mut metrics = self.metrics.lock().await;
        metrics.messages_updated += 1;
        
        // Update rolling average
        let new_time_ms = duration.as_millis() as f64;
        if metrics.average_update_time_ms == 0.0 {
            metrics.average_update_time_ms = new_time_ms;
        } else {
            metrics.average_update_time_ms = (metrics.average_update_time_ms * 0.9) + (new_time_ms * 0.1);
        }
    }

    /// Record a rate limit hit
    pub async fn record_rate_limit_hit(&self) {
        let mut metrics = self.metrics.lock().await;
        metrics.rate_limit_hits += 1;
    }

    /// Record a failed update
    pub async fn record_failed_update(&self) {
        let mut metrics = self.metrics.lock().await;
        metrics.failed_updates += 1;
    }

    /// Get current metrics snapshot
    pub async fn get_metrics(&self) -> PerformanceMetrics {
        self.metrics.lock().await.clone()
    }

    /// Store metrics to database
    pub async fn store_metrics(&self) -> Result<()> {
        let metrics = self.get_metrics().await;
        
        sqlx::query(
            "INSERT INTO performance_metrics 
             (timestamp, messages_updated, average_update_time_ms, queue_size, rate_limit_hits, failed_updates) 
             VALUES (?, ?, ?, ?, ?, ?)"
        )
        .bind(metrics.timestamp)
        .bind(metrics.messages_updated as i64)
        .bind(metrics.average_update_time_ms)
        .bind(metrics.queue_size as i64)
        .bind(metrics.rate_limit_hits as i64)
        .bind(metrics.failed_updates as i64)
        .execute(&self.db)
        .await?;

        Ok(())
    }
}