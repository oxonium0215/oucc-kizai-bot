use anyhow::Result;
use chrono::{Utc, Duration as ChronoDuration};
use rand::{thread_rng, seq::SliceRandom};
use serenity::prelude::*;
use serenity::model::prelude::*;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::info;

use crate::models::{Equipment, Tag};
use crate::performance::{MessageUpdateManager, MessageUpdate, UpdatePriority, PerformanceMetrics, MetricsCollector};

/// Configuration for load testing scenarios
#[derive(Debug, Clone)]
pub struct LoadTestConfig {
    /// Number of equipment items to create
    pub equipment_count: usize,
    /// Number of concurrent users simulated
    pub concurrent_users: usize,
    /// Number of tags to create
    pub tag_count: usize,
    /// Duration of the load test
    pub test_duration: Duration,
    /// Interval between user actions
    pub user_action_interval: Duration,
    /// Probability of creating reservations (0.0 - 1.0)
    pub reservation_probability: f64,
}

impl Default for LoadTestConfig {
    fn default() -> Self {
        Self {
            equipment_count: 100,
            concurrent_users: 20,
            tag_count: 10,
            test_duration: Duration::from_secs(300), // 5 minutes
            user_action_interval: Duration::from_secs(5),
            reservation_probability: 0.7,
        }
    }
}

/// Load test results and statistics
#[derive(Debug, Clone)]
pub struct LoadTestResults {
    pub equipment_created: usize,
    pub reservations_created: usize,
    pub message_updates_sent: usize,
    pub total_duration: Duration,
    pub average_response_time: Duration,
    pub peak_queue_size: usize,
    pub rate_limit_hits: usize,
    pub errors: usize,
    pub performance_metrics: Vec<PerformanceMetrics>,
}

/// Simulated user for load testing
struct SimulatedUser {
    user_id: u64,
    actions_performed: usize,
    last_action_time: Instant,
}

/// Load testing framework for equipment management
pub struct LoadTester {
    db: SqlitePool,
    guild_id: i64,
    channel_id: i64,
    config: LoadTestConfig,
    message_manager: MessageUpdateManager,
    metrics_collector: MetricsCollector,
}

impl LoadTester {
    pub fn new(
        db: SqlitePool,
        guild_id: i64,
        channel_id: i64,
        config: Option<LoadTestConfig>,
    ) -> Self {
        let config = config.unwrap_or_default();
        let message_manager = MessageUpdateManager::new(None);
        let metrics_collector = MetricsCollector::new(db.clone());

        Self {
            db,
            guild_id,
            channel_id,
            config,
            message_manager,
            metrics_collector,
        }
    }

    /// Run comprehensive load test
    pub async fn run_load_test(&self, ctx: Arc<Context>) -> Result<LoadTestResults> {
        info!("Starting load test with {} equipment items and {} users", 
               self.config.equipment_count, self.config.concurrent_users);

        let start_time = Instant::now();
        let mut results = LoadTestResults {
            equipment_created: 0,
            reservations_created: 0,
            message_updates_sent: 0,
            total_duration: Duration::ZERO,
            average_response_time: Duration::ZERO,
            peak_queue_size: 0,
            rate_limit_hits: 0,
            errors: 0,
            performance_metrics: Vec::new(),
        };

        // Start message update processor
        self.message_manager.start_processor(ctx.clone()).await?;

        // Phase 1: Setup test data
        info!("Phase 1: Creating test data");
        let (tags, equipment) = self.setup_test_data().await?;
        results.equipment_created = equipment.len();

        // Phase 2: Simulate concurrent user activity
        info!("Phase 2: Simulating {} concurrent users", self.config.concurrent_users);
        let user_results = self.simulate_user_activity(&equipment, &tags, ctx.clone()).await?;
        results.reservations_created = user_results.reservations_created;
        results.message_updates_sent = user_results.message_updates_sent;
        results.errors = user_results.errors;

        // Phase 3: Stress test message updates
        info!("Phase 3: Stress testing message updates");
        let message_results = self.stress_test_messages(&equipment, ctx.clone()).await?;
        results.message_updates_sent += message_results.message_updates_sent;
        results.peak_queue_size = message_results.peak_queue_size;
        results.rate_limit_hits = message_results.rate_limit_hits;

        // Stop message processor and collect final metrics
        self.message_manager.stop_processor().await;
        
        results.total_duration = start_time.elapsed();
        results.performance_metrics = self.collect_performance_metrics().await?;

        // Cleanup test data
        self.cleanup_test_data().await?;

        info!("Load test completed in {}ms", results.total_duration.as_millis());
        Ok(results)
    }

    /// Setup test data (equipment, tags, locations)
    pub async fn setup_test_data(&self) -> Result<(Vec<Tag>, Vec<Equipment>)> {
        let mut tags = Vec::new();
        let mut equipment = Vec::new();

        // Create tags
        for i in 0..self.config.tag_count {
            let tag_name = format!("TestTag{}", i + 1);
            let tag_id: i64 = sqlx::query_scalar(
                "INSERT INTO tags (guild_id, name, sort_order) VALUES (?, ?, ?) RETURNING id"
            )
            .bind(self.guild_id)
            .bind(&tag_name)
            .bind(i as i64)
            .fetch_one(&self.db)
            .await?;

            tags.push(Tag {
                id: tag_id,
                guild_id: self.guild_id,
                name: tag_name,
                sort_order: i as i64,
                created_at: Utc::now(),
            });
        }

        // Create equipment
        let mut rng = thread_rng();
        for i in 0..self.config.equipment_count {
            let tag = tags.choose(&mut rng);
            let equipment_name = format!("TestEquipment{:03}", i + 1);
            
            let equipment_id: i64 = sqlx::query_scalar(
                "INSERT INTO equipment (guild_id, tag_id, name, status, current_location) 
                 VALUES (?, ?, ?, ?, ?) RETURNING id"
            )
            .bind(self.guild_id)
            .bind(tag.map(|t| t.id))
            .bind(&equipment_name)
            .bind("Available")
            .bind(Some("Test Location"))
            .fetch_one(&self.db)
            .await?;

            equipment.push(Equipment {
                id: equipment_id,
                guild_id: self.guild_id,
                tag_id: tag.map(|t| t.id),
                name: equipment_name,
                status: "Available".to_string(),
                current_location: Some("Test Location".to_string()),
                unavailable_reason: None,
                default_return_location: Some("Test Location".to_string()),
                message_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            });
        }

        info!("Created {} tags and {} equipment items", tags.len(), equipment.len());
        Ok((tags, equipment))
    }

    /// Simulate concurrent user activity
    async fn simulate_user_activity(
        &self,
        equipment: &[Equipment],
        _tags: &[Tag],
        ctx: Arc<Context>,
    ) -> Result<UserActivityResults> {
        let mut users = Vec::new();
        let mut results = UserActivityResults {
            reservations_created: 0,
            message_updates_sent: 0,
            errors: 0,
        };

        // Create simulated users
        for i in 0..self.config.concurrent_users {
            users.push(SimulatedUser {
                user_id: 1000000 + i as u64,
                actions_performed: 0,
                last_action_time: Instant::now(),
            });
        }

        let test_end_time = Instant::now() + self.config.test_duration;
        let mut rng = thread_rng();

        while Instant::now() < test_end_time {
            // Spawn concurrent user actions
            let mut handles = Vec::new();
            
            for user in &mut users {
                if user.last_action_time.elapsed() >= self.config.user_action_interval {
                    let equipment_item = equipment.choose(&mut rng).unwrap().clone();
                    let user_id = user.user_id;
                    let db = self.db.clone();
                    let message_manager = self.message_manager.clone();
                    let ctx = ctx.clone();
                    let channel_id = ChannelId::new(self.channel_id as u64);
                    
                    let handle = tokio::spawn(async move {
                        Self::perform_user_action(user_id, &equipment_item, &db, &message_manager, ctx, channel_id).await
                    });
                    
                    handles.push(handle);
                    user.last_action_time = Instant::now();
                    user.actions_performed += 1;
                }
            }

            // Wait for actions to complete
            for handle in handles {
                match handle.await? {
                    Ok(action_result) => {
                        if action_result.reservation_created {
                            results.reservations_created += 1;
                        }
                        if action_result.message_updated {
                            results.message_updates_sent += 1;
                        }
                    }
                    Err(_) => {
                        results.errors += 1;
                    }
                }
            }

            sleep(Duration::from_millis(100)).await;
        }

        info!("User simulation completed: {} reservations, {} message updates, {} errors",
               results.reservations_created, results.message_updates_sent, results.errors);

        Ok(results)
    }

    /// Perform a single user action (create/modify reservation)
    async fn perform_user_action(
        user_id: u64,
        equipment: &Equipment,
        db: &SqlitePool,
        message_manager: &MessageUpdateManager,
        _ctx: Arc<Context>,
        channel_id: ChannelId,
    ) -> Result<UserActionResult> {
        let mut result = UserActionResult {
            reservation_created: false,
            message_updated: false,
        };

        // Check if equipment is available
        let current_status: String = sqlx::query_scalar(
            "SELECT status FROM equipment WHERE id = ?"
        )
        .bind(equipment.id)
        .fetch_one(db)
        .await?;

        if current_status == "Available" {
            // Use a simple probability check without thread_rng in async context
            let should_create_reservation = (user_id % 10) < 7; // 70% probability

            if should_create_reservation {
                // Create reservation
                let start_time = Utc::now() + ChronoDuration::hours(1);
                let duration_hours = 1 + (user_id % 48); // 1-48 hours
                let end_time = start_time + ChronoDuration::hours(duration_hours as i64);

                sqlx::query(
                    "INSERT INTO reservations (equipment_id, user_id, start_time, end_time, status) 
                     VALUES (?, ?, ?, ?, ?)"
                )
                .bind(equipment.id)
                .bind(user_id as i64)
                .bind(start_time)
                .bind(end_time)
                .bind("Confirmed")
                .execute(db)
                .await?;

                // Update equipment status
                sqlx::query(
                    "UPDATE equipment SET status = 'Loaned', updated_at = CURRENT_TIMESTAMP WHERE id = ?"
                )
                .bind(equipment.id)
                .execute(db)
                .await?;

                result.reservation_created = true;

                // Queue message update
                if let Some(message_id) = equipment.message_id {
                    let update = MessageUpdate {
                        guild_id: GuildId::new(equipment.guild_id as u64),
                        channel_id,
                        message_id: MessageId::new(message_id as u64),
                        embed: Some(Self::create_test_embed(equipment, "Loaned")),
                        components: vec![],
                        priority: UpdatePriority::Normal,
                        created_at: Instant::now(),
                        retries: 0,
                    };

                    message_manager.queue_update(update).await;
                    result.message_updated = true;
                }
            }
        }

        Ok(result)
    }

    /// Stress test message update system
    async fn stress_test_messages(
        &self,
        equipment: &[Equipment],
        ctx: Arc<Context>,
    ) -> Result<MessageStressResults> {
        info!("Starting message update stress test");
        
        let mut results = MessageStressResults {
            message_updates_sent: 0,
            peak_queue_size: 0,
            rate_limit_hits: 0,
        };

        let channel_id = ChannelId::new(self.channel_id as u64);
        let guild_id = GuildId::new(self.guild_id as u64);

        // Create dummy messages for testing
        let mut test_messages = Vec::new();
        for (_i, equip) in equipment.iter().take(50).enumerate() {
            let embed = Self::create_test_embed(equip, "Available");
            
            let message = channel_id.send_message(&ctx.http,
                serenity::all::CreateMessage::new().embed(embed)
            ).await?;
            
            test_messages.push(message.id);
            
            // Update equipment with message ID
            sqlx::query(
                "UPDATE equipment SET message_id = ? WHERE id = ?"
            )
            .bind(message.id.get() as i64)
            .bind(equip.id)
            .execute(&self.db)
            .await?;
        }

        // Flood the message update queue
        for _ in 0..1000 {
            for (i, &message_id) in test_messages.iter().enumerate() {
                let equipment = &equipment[i];
                
                let update = MessageUpdate {
                    guild_id,
                    channel_id,
                    message_id,
                    embed: Some(Self::create_test_embed(equipment, "Stress Test")),
                    components: vec![],
                    priority: if i % 10 == 0 { UpdatePriority::High } else { UpdatePriority::Normal },
                    created_at: Instant::now(),
                    retries: 0,
                };

                self.message_manager.queue_update(update).await;
                results.message_updates_sent += 1;
            }

            // Monitor queue size
            let stats = self.message_manager.get_stats().await;
            results.peak_queue_size = results.peak_queue_size.max(stats.queue_size);
            results.rate_limit_hits = stats.consecutive_failures;

            sleep(Duration::from_millis(10)).await;
        }

        // Wait for queue to drain
        let start_time = Instant::now();
        while start_time.elapsed() < Duration::from_secs(60) {
            let stats = self.message_manager.get_stats().await;
            if stats.queue_size == 0 {
                break;
            }
            sleep(Duration::from_millis(500)).await;
        }

        // Cleanup test messages
        for message_id in test_messages {
            let _ = channel_id.delete_message(&ctx.http, message_id).await;
        }

        info!("Message stress test completed");
        Ok(results)
    }

    /// Create a test embed for equipment
    fn create_test_embed(equipment: &Equipment, status: &str) -> serenity::all::CreateEmbed {
        serenity::all::CreateEmbed::new()
            .title(&equipment.name)
            .description(format!("Status: {}", status))
            .field("ID", equipment.id.to_string(), true)
            .field("Location", equipment.current_location.as_deref().unwrap_or("Unknown"), true)
            .color(match status {
                "Available" => serenity::all::Colour::DARK_GREEN,
                "Loaned" => serenity::all::Colour::ORANGE,
                _ => serenity::all::Colour::BLUE,
            })
    }

    /// Collect performance metrics during test
    async fn collect_performance_metrics(&self) -> Result<Vec<PerformanceMetrics>> {
        // In a real implementation, this would collect metrics over time
        // For now, return current snapshot
        Ok(vec![self.metrics_collector.get_metrics().await])
    }

    /// Cleanup all test data
    pub async fn cleanup_test_data(&self) -> Result<()> {
        info!("Cleaning up test data");

        // Delete test reservations
        sqlx::query("DELETE FROM reservations WHERE user_id >= 1000000")
            .execute(&self.db)
            .await?;

        // Delete test equipment
        sqlx::query("DELETE FROM equipment WHERE guild_id = ? AND name LIKE 'TestEquipment%'")
            .bind(self.guild_id)
            .execute(&self.db)
            .await?;

        // Delete test tags
        sqlx::query("DELETE FROM tags WHERE guild_id = ? AND name LIKE 'TestTag%'")
            .bind(self.guild_id)
            .execute(&self.db)
            .await?;

        info!("Test data cleanup completed");
        Ok(())
    }
}

#[derive(Debug)]
struct UserActivityResults {
    reservations_created: usize,
    message_updates_sent: usize,
    errors: usize,
}

#[derive(Debug)]
struct UserActionResult {
    reservation_created: bool,
    message_updated: bool,
}

#[derive(Debug)]
struct MessageStressResults {
    message_updates_sent: usize,
    peak_queue_size: usize,
    rate_limit_hits: usize,
}

/// Load testing command for manual execution
pub async fn run_load_test_command(
    ctx: &Context,
    guild_id: GuildId,
    channel_id: ChannelId,
    db: &SqlitePool,
    config: Option<LoadTestConfig>,
) -> Result<String> {
    let load_tester = LoadTester::new(
        db.clone(),
        guild_id.get() as i64,
        channel_id.get() as i64,
        config,
    );

    let results = load_tester.run_load_test(Arc::new(ctx.clone())).await?;

    Ok(format!(
        "**Load Test Results**\n\
         Equipment Created: {}\n\
         Reservations Created: {}\n\
         Message Updates: {}\n\
         Test Duration: {}ms\n\
         Peak Queue Size: {}\n\
         Rate Limit Hits: {}\n\
         Errors: {}",
        results.equipment_created,
        results.reservations_created,
        results.message_updates_sent,
        results.total_duration.as_millis(),
        results.peak_queue_size,
        results.rate_limit_hits,
        results.errors
    ))
}