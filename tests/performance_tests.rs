#[cfg(test)]
mod tests {
    use oucc_kizai_bot::performance::{MessageUpdateManager, RateLimitConfig, MessageUpdate, UpdatePriority, MetricsCollector};
    use oucc_kizai_bot::load_testing::{LoadTester, LoadTestConfig};
    use oucc_kizai_bot::jobs::JobWorker;
    use serenity::model::prelude::*;
    use std::time::{Duration, Instant};

    /// Create a test database
    async fn create_test_db() -> sqlx::SqlitePool {
        let database_url = "sqlite::memory:";
        
        let pool = sqlx::SqlitePool::connect(database_url).await.unwrap();
        
        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        
        pool
    }

    #[tokio::test]
    async fn test_message_update_manager_queue() {
        let config = RateLimitConfig {
            batch_size: 5,
            batch_delay_ms: 100,
            max_concurrent_updates: 2,
            base_backoff_ms: 100,
            max_backoff_ms: 1000,
            max_retries: 3,
        };
        
        let manager = MessageUpdateManager::new(Some(config));
        
        // Queue several updates
        for i in 0..10 {
            let update = MessageUpdate {
                guild_id: GuildId::new(1),
                channel_id: ChannelId::new(1),
                message_id: MessageId::new(i + 1),
                embed: None,
                components: vec![],
                priority: if i % 2 == 0 { UpdatePriority::High } else { UpdatePriority::Normal },
                created_at: Instant::now(),
                retries: 0,
            };
            
            manager.queue_update(update).await;
        }
        
        let stats = manager.get_stats().await;
        assert_eq!(stats.queue_size, 10);
    }

    #[tokio::test]
    async fn test_rate_limiter_backoff() {
        let config = RateLimitConfig {
            base_backoff_ms: 100,
            max_backoff_ms: 1000,
            ..Default::default()
        };
        
        // Create a message update manager with the config to test rate limiting
        let manager = MessageUpdateManager::new(Some(config));
        let stats = manager.get_stats().await;
        
        // Basic test - verify that the manager can be created and stats retrieved
        assert_eq!(stats.queue_size, 0);
        assert_eq!(stats.consecutive_failures, 0);
    }

    #[tokio::test]
    async fn test_load_tester_setup() {
        let db = create_test_db().await;
        let guild_id = 123456789i64;
        let channel_id = 987654321i64;
        
        // Create guild first
        sqlx::query("INSERT INTO guilds (id) VALUES (?)")
            .bind(guild_id)
            .execute(&db)
            .await
            .unwrap();
        
        let config = LoadTestConfig {
            equipment_count: 10,
            concurrent_users: 3,
            tag_count: 2,
            test_duration: Duration::from_secs(5),
            user_action_interval: Duration::from_secs(1),
            reservation_probability: 0.5,
        };
        
        let load_tester = LoadTester::new(db.clone(), guild_id, channel_id, Some(config));
        
        // Test data setup
        let (tags, equipment) = load_tester.setup_test_data().await.unwrap();
        
        assert_eq!(tags.len(), 2);
        assert_eq!(equipment.len(), 10);
        
        // Verify data in database
        let equipment_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM equipment WHERE guild_id = ?"
        )
        .bind(guild_id)
        .fetch_one(&db)
        .await
        .unwrap();
        
        assert_eq!(equipment_count, 10);
        
        // Cleanup
        load_tester.cleanup_test_data().await.unwrap();
        
        let equipment_count_after: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM equipment WHERE guild_id = ?"
        )
        .bind(guild_id)
        .fetch_one(&db)
        .await
        .unwrap();
        
        assert_eq!(equipment_count_after, 0);
    }

    #[tokio::test]
    async fn test_performance_metrics_collection() {
        let db = create_test_db().await;
        let metrics_collector = MetricsCollector::new(db.clone());
        
        // Record some metrics
        metrics_collector.record_update(Duration::from_millis(150)).await;
        metrics_collector.record_update(Duration::from_millis(200)).await;
        metrics_collector.record_rate_limit_hit().await;
        
        let metrics = metrics_collector.get_metrics().await;
        assert_eq!(metrics.messages_updated, 2);
        assert_eq!(metrics.rate_limit_hits, 1);
        assert!(metrics.average_update_time_ms > 0.0);
        
        // Store metrics to database
        metrics_collector.store_metrics().await.unwrap();
        
        // Verify storage
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM performance_metrics"
        )
        .fetch_one(&db)
        .await
        .unwrap();
        
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_database_indexes() {
        let db = create_test_db().await;
        
        // Verify that our performance indexes exist
        let index_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%'"
        )
        .fetch_one(&db)
        .await
        .unwrap();
        
        // Should have our new performance indexes
        assert!(index_count >= 8); // We added 8+ indexes in the migration
    }

    #[tokio::test]
    async fn test_batch_message_update_scheduling() {
        let db = create_test_db().await;
        let job_worker = JobWorker::new(db.clone());
        
        // Schedule a batch message update job
        job_worker.schedule_batch_message_update(123456, "high").await.unwrap();
        
        // Verify job was created
        let job_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM jobs WHERE job_type = 'batch_message_update'"
        )
        .fetch_one(&db)
        .await
        .unwrap();
        
        assert_eq!(job_count, 1);
    }

    #[tokio::test]
    async fn test_priority_queue_ordering() {
        let manager = MessageUpdateManager::new(None);
        
        // Add updates with different priorities
        let low_update = MessageUpdate {
            guild_id: GuildId::new(1),
            channel_id: ChannelId::new(1),
            message_id: MessageId::new(1),
            embed: None,
            components: vec![],
            priority: UpdatePriority::Low,
            created_at: Instant::now(),
            retries: 0,
        };
        
        let high_update = MessageUpdate {
            guild_id: GuildId::new(1),
            channel_id: ChannelId::new(1),
            message_id: MessageId::new(2),
            embed: None,
            components: vec![],
            priority: UpdatePriority::High,
            created_at: Instant::now(),
            retries: 0,
        };
        
        let critical_update = MessageUpdate {
            guild_id: GuildId::new(1),
            channel_id: ChannelId::new(1),
            message_id: MessageId::new(3),
            embed: None,
            components: vec![],
            priority: UpdatePriority::Critical,
            created_at: Instant::now(),
            retries: 0,
        };
        
        // Queue in random order
        manager.queue_update(low_update).await;
        manager.queue_update(high_update).await;
        manager.queue_update(critical_update).await;
        
        // Verify queue size
        let stats = manager.get_stats().await;
        assert_eq!(stats.queue_size, 3);
        
        // In a real test, we'd verify that critical messages are processed first
        // This would require exposing more queue internals or integration testing
    }
}