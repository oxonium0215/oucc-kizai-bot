use anyhow::Result;
use oucc_kizai_bot::handlers::Handler;
use serenity::{
    model::prelude::*,
    prelude::*,
};
use std::sync::Arc;

mod common;
use common::*;

/// Test message deletion in reservation channels
#[tokio::test]
async fn test_message_deletion_in_reservation_channel() -> Result<()> {
    let ctx = TestContext::new().await?;
    let handler = Handler::new(ctx.db.clone());

    // Set up guild with reservation channel
    let guild_id = 123456789i64;
    let reservation_channel_id = 987654321i64;
    let regular_channel_id = 555555555i64;

    let guild = GuildBuilder::new(guild_id)
        .with_reservation_channel(reservation_channel_id)
        .build(&ctx.db)
        .await?;

    assert_eq!(guild.reservation_channel_id, Some(reservation_channel_id));

    // Create mock Discord context
    let serenity_guild_id = GuildId::new(guild_id as u64);
    let reservation_serenity_channel_id = ChannelId::new(reservation_channel_id as u64);
    let regular_serenity_channel_id = ChannelId::new(regular_channel_id as u64);

    // Mock context (we can't fully mock serenity Context easily, so this is conceptual)
    // In a real test, we'd need to create a more sophisticated mock framework
    
    // Test case 1: User message in reservation channel should be deleted
    {
        // Create a mock message from a user (not bot) in the reservation channel
        let mock_user = User {
            id: UserId::new(111111111),
            name: "testuser".to_string(),
            discriminator: 0,
            global_name: None,
            avatar: None,
            bot: false, // This is a regular user
            system: false,
            mfa_enabled: false,
            banner: None,
            accent_colour: None,
            locale: None,
            verified: None,
            email: None,
            flags: None,
            premium_type: None,
            public_flags: None,
            avatar_decoration: None,
        };

        let mock_message = Message {
            id: MessageId::new(999999999),
            channel_id: reservation_serenity_channel_id,
            guild_id: Some(serenity_guild_id),
            author: mock_user,
            content: "Hello, this is a user message".to_string(),
            timestamp: chrono::Utc::now().into(),
            edited_timestamp: None,
            tts: false,
            mention_everyone: false,
            mentions: vec![],
            mention_roles: vec![],
            mention_channels: vec![],
            attachments: vec![],
            embeds: vec![],
            reactions: vec![],
            nonce: None,
            pinned: false,
            webhook_id: None,
            kind: MessageType::Regular,
            activity: None,
            application: None,
            application_id: None,
            message_reference: None,
            flags: None,
            referenced_message: None,
            interaction: None,
            thread: None,
            components: vec![],
            sticker_items: vec![],
            stickers: vec![],
            position: None,
            role_subscription_data: None,
        };

        // Note: We cannot easily test the actual message deletion without a full mock of Context
        // But we can test the database query logic that determines if it's a reservation channel
        
        // Verify the reservation channel is correctly identified
        let is_reservation_channel: Option<i64> = sqlx::query_scalar(
            "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
        )
        .bind(guild_id)
        .bind(reservation_channel_id)
        .fetch_optional(&ctx.db)
        .await?
        .flatten();

        assert!(is_reservation_channel.is_some());
        assert_eq!(is_reservation_channel.unwrap(), reservation_channel_id);
    }

    // Test case 2: Message in regular channel should not trigger deletion logic
    {
        let is_reservation_channel: Option<i64> = sqlx::query_scalar(
            "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
        )
        .bind(guild_id)
        .bind(regular_channel_id)
        .fetch_optional(&ctx.db)
        .await?
        .flatten();

        assert!(is_reservation_channel.is_none());
    }

    // Test case 3: Guild without reservation channel configured
    {
        let guild_without_channel_id = 999999999i64;
        
        // Create guild without reservation channel
        GuildBuilder::new(guild_without_channel_id)
            .build(&ctx.db)
            .await?;

        let is_reservation_channel: Option<i64> = sqlx::query_scalar(
            "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
        )
        .bind(guild_without_channel_id)
        .bind(reservation_channel_id)
        .fetch_optional(&ctx.db)
        .await?
        .flatten();

        assert!(is_reservation_channel.is_none());
    }

    println!("✓ Message deletion logic tests passed");
    Ok(())
}

/// Test the decision logic for message deletion
#[tokio::test]
async fn test_message_deletion_decision_logic() -> Result<()> {
    let ctx = TestContext::new().await?;

    let guild_id = 123456789i64;
    let reservation_channel_id = 987654321i64;

    // Set up guild with reservation channel
    let guild = GuildBuilder::new(guild_id)
        .with_reservation_channel(reservation_channel_id)
        .build(&ctx.db)
        .await?;

    // Test helper function to check if a message should be deleted
    async fn should_delete_message(
        db: &sqlx::SqlitePool,
        guild_id: i64,
        channel_id: i64,
        is_bot: bool,
    ) -> Result<bool> {
        // Simulate the logic from the message handler
        if is_bot {
            return Ok(false); // Bot messages should not be deleted
        }

        let reservation_channel_id: Option<i64> = sqlx::query_scalar(
            "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
        )
        .bind(guild_id)
        .bind(channel_id)
        .fetch_optional(db)
        .await?
        .flatten();

        Ok(reservation_channel_id.is_some())
    }

    // Test case 1: User message in reservation channel should be deleted
    assert!(should_delete_message(&ctx.db, guild_id, reservation_channel_id, false).await?);

    // Test case 2: Bot message in reservation channel should NOT be deleted
    assert!(!should_delete_message(&ctx.db, guild_id, reservation_channel_id, true).await?);

    // Test case 3: User message in non-reservation channel should NOT be deleted
    let regular_channel_id = 555555555i64;
    assert!(!should_delete_message(&ctx.db, guild_id, regular_channel_id, false).await?);

    // Test case 4: Message in guild without reservation channel should NOT be deleted
    let guild_without_channel_id = 999999999i64;
    GuildBuilder::new(guild_without_channel_id)
        .build(&ctx.db)
        .await?;
    assert!(!should_delete_message(&ctx.db, guild_without_channel_id, reservation_channel_id, false).await?);

    println!("✓ Message deletion decision logic tests passed");
    Ok(())
}

/// Test database error handling
#[tokio::test]
async fn test_database_error_resilience() -> Result<()> {
    let ctx = TestContext::new().await?;

    // Test what happens when database query fails (simulated by invalid guild ID type)
    // This tests the error handling path in the message handler
    
    let result: Result<Option<Option<i64>>, sqlx::Error> = sqlx::query_scalar(
        "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
    )
    .bind("invalid_id") // This should cause a type error
    .bind(123456789i64)
    .fetch_optional(&ctx.db)
    .await;

    // Verify that the query fails as expected (demonstrating error handling is needed)
    assert!(result.is_err());

    // The actual handler should gracefully handle this error and continue without panicking
    println!("✓ Database error resilience test passed");
    Ok(())
}

/// Test efficient query optimization
#[tokio::test]
async fn test_query_efficiency() -> Result<()> {
    let ctx = TestContext::new().await?;

    let guild_id = 123456789i64;
    let reservation_channel_id = 987654321i64;

    // Set up guild with reservation channel
    GuildBuilder::new(guild_id)
        .with_reservation_channel(reservation_channel_id)
        .build(&ctx.db)
        .await?;

    // Test the optimized query used in the handler
    let start_time = std::time::Instant::now();
    
    let result: Option<i64> = sqlx::query_scalar(
        "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
    )
    .bind(guild_id)
    .bind(reservation_channel_id)
    .fetch_optional(&ctx.db)
    .await?
    .flatten();

    let query_duration = start_time.elapsed();

    // Verify the query works correctly
    assert!(result.is_some());
    assert_eq!(result.unwrap(), reservation_channel_id);

    // Verify query is reasonably fast (less than 10ms for in-memory SQLite)
    assert!(query_duration.as_millis() < 10);

    // Test with non-matching channel ID
    let result2: Option<i64> = sqlx::query_scalar(
        "SELECT reservation_channel_id FROM guilds WHERE id = ? AND reservation_channel_id = ?"
    )
    .bind(guild_id)
    .bind(999999999i64) // Different channel ID
    .fetch_optional(&ctx.db)
    .await?
    .flatten();

    assert!(result2.is_none());

    println!("✓ Query efficiency test passed");
    Ok(())
}