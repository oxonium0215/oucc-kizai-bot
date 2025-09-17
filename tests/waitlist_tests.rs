use anyhow::Result;
use chrono::{DateTime, Utc, Duration};
use oucc_kizai_bot::waitlist::*;
use oucc_kizai_bot::models::*;
use sqlx::SqlitePool;

mod common;

#[tokio::test]
async fn test_waitlist_fifo_ordering() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Apply all migrations including waitlist
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let waitlist_manager = WaitlistManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let equipment_id = 1i64;
    
    // Create test equipment first
    sqlx::query!(
        "INSERT INTO equipment (id, guild_id, name, status) VALUES (?, ?, ?, ?)",
        equipment_id,
        guild_id,
        "Test Equipment",
        "Available"
    )
    .execute(&ctx.db)
    .await?;
    
    let now = Utc::now();
    let desired_start = now + Duration::hours(1);
    let desired_end = now + Duration::hours(3);
    
    // Multiple users join waitlist for same time window
    let user1 = 100001i64;
    let user2 = 100002i64;
    let user3 = 100003i64;
    
    // User 1 joins first
    let result1 = waitlist_manager.join_waitlist(guild_id, equipment_id, user1, desired_start, desired_end).await?;
    
    // User 2 joins second (slight delay to ensure ordering)
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let result2 = waitlist_manager.join_waitlist(guild_id, equipment_id, user2, desired_start, desired_end).await?;
    
    // User 3 joins third
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    let result3 = waitlist_manager.join_waitlist(guild_id, equipment_id, user3, desired_start, desired_end).await?;
    
    // All should succeed
    assert!(matches!(result1, WaitlistJoinResult::Success(_)));
    assert!(matches!(result2, WaitlistJoinResult::Success(_)));
    assert!(matches!(result3, WaitlistJoinResult::Success(_)));
    
    // Get waitlist entries in order
    let entries = waitlist_manager.get_equipment_waitlist_entries(equipment_id).await?;
    assert_eq!(entries.len(), 3);
    
    // Should be in FIFO order (first joined = first in queue)
    assert_eq!(entries[0].user_id, user1);
    assert_eq!(entries[1].user_id, user2);
    assert_eq!(entries[2].user_id, user3);
    
    // Create offer for available window - should go to user1 (first in queue)
    let offer_result = waitlist_manager.create_offer_for_available_window(
        equipment_id,
        desired_start,
        desired_end,
        guild_id,
    ).await?;
    
    assert!(offer_result.is_some());
    let offer = offer_result.unwrap();
    assert_eq!(offer.waitlist_entry.user_id, user1);
    
    Ok(())
}

#[tokio::test]
async fn test_waitlist_duplicate_prevention() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let waitlist_manager = WaitlistManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let equipment_id = 1i64;
    let user_id = 100001i64;
    
    // Create test equipment
    sqlx::query!(
        "INSERT INTO equipment (id, guild_id, name, status) VALUES (?, ?, ?, ?)",
        equipment_id,
        guild_id,
        "Test Equipment",
        "Available"
    )
    .execute(&ctx.db)
    .await?;
    
    let now = Utc::now();
    let desired_start = now + Duration::hours(1);
    let desired_end = now + Duration::hours(3);
    
    // First join should succeed
    let result1 = waitlist_manager.join_waitlist(guild_id, equipment_id, user_id, desired_start, desired_end).await?;
    assert!(matches!(result1, WaitlistJoinResult::Success(_)));
    
    // Second join for same user/equipment/window should be rejected
    let result2 = waitlist_manager.join_waitlist(guild_id, equipment_id, user_id, desired_start, desired_end).await?;
    assert!(matches!(result2, WaitlistJoinResult::AlreadyExists(_)));
    
    Ok(())
}

#[tokio::test]
async fn test_waitlist_hold_conflict_detection() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let waitlist_manager = WaitlistManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let equipment_id = 1i64;
    let user1 = 100001i64;
    let user2 = 100002i64;
    
    // Create test equipment and guild with offer hold settings
    sqlx::query!(
        "INSERT INTO guilds (id, offer_hold_minutes) VALUES (?, ?)",
        guild_id,
        15
    )
    .execute(&ctx.db)
    .await?;
    
    sqlx::query!(
        "INSERT INTO equipment (id, guild_id, name, status) VALUES (?, ?, ?, ?)",
        equipment_id,
        guild_id,
        "Test Equipment",
        "Available"
    )
    .execute(&ctx.db)
    .await?;
    
    let now = Utc::now();
    let desired_start = now + Duration::hours(1);
    let desired_end = now + Duration::hours(3);
    
    // User 1 joins waitlist
    let result1 = waitlist_manager.join_waitlist(guild_id, equipment_id, user1, desired_start, desired_end).await?;
    assert!(matches!(result1, WaitlistJoinResult::Success(_)));
    
    // Create an offer for user 1 (this creates a hold)
    let offer_result = waitlist_manager.create_offer_for_available_window(
        equipment_id,
        desired_start,
        desired_end,
        guild_id,
    ).await?;
    
    assert!(offer_result.is_some());
    let offer = offer_result.unwrap();
    
    // Check that there's a hold for user 2 (should find the hold)
    let hold_check = waitlist_manager.check_waitlist_hold(
        equipment_id,
        desired_start,
        desired_end,
        Some(user2), // Exclude user2, so should find user1's hold
    ).await?;
    
    assert!(hold_check.is_some());
    
    // Check that there's no hold for user 1 (should not find hold since user1 is excluded)
    let no_hold_check = waitlist_manager.check_waitlist_hold(
        equipment_id,
        desired_start,
        desired_end,
        Some(user1), // Exclude user1, so should not find their own hold
    ).await?;
    
    assert!(no_hold_check.is_none());
    
    Ok(())
}

#[tokio::test]
async fn test_waitlist_offer_acceptance() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let waitlist_manager = WaitlistManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let equipment_id = 1i64;
    let user_id = 100001i64;
    
    // Create test data
    sqlx::query!(
        "INSERT INTO guilds (id, offer_hold_minutes) VALUES (?, ?)",
        guild_id,
        15
    )
    .execute(&ctx.db)
    .await?;
    
    sqlx::query!(
        "INSERT INTO equipment (id, guild_id, name, status) VALUES (?, ?, ?, ?)",
        equipment_id,
        guild_id,
        "Test Equipment",
        "Available"
    )
    .execute(&ctx.db)
    .await?;
    
    let now = Utc::now();
    let desired_start = now + Duration::hours(1);
    let desired_end = now + Duration::hours(3);
    
    // User joins waitlist
    let join_result = waitlist_manager.join_waitlist(guild_id, equipment_id, user_id, desired_start, desired_end).await?;
    assert!(matches!(join_result, WaitlistJoinResult::Success(_)));
    
    // Create offer
    let offer_result = waitlist_manager.create_offer_for_available_window(
        equipment_id,
        desired_start,
        desired_end,
        guild_id,
    ).await?;
    
    assert!(offer_result.is_some());
    let offer = offer_result.unwrap();
    
    // Accept the offer
    let acceptance_result = waitlist_manager.accept_offer(offer.offer_id, user_id).await?;
    assert!(acceptance_result.is_some());
    
    let reservation_id = acceptance_result.unwrap();
    assert!(reservation_id > 0);
    
    // Verify reservation was created
    let reservation = sqlx::query!(
        "SELECT * FROM reservations WHERE id = ?",
        reservation_id
    )
    .fetch_optional(&ctx.db)
    .await?;
    
    assert!(reservation.is_some());
    let res = reservation.unwrap();
    assert_eq!(res.user_id, user_id);
    assert_eq!(res.equipment_id, equipment_id);
    assert_eq!(res.status.unwrap_or_default(), "Confirmed");
    
    // Verify waitlist entry was cancelled
    let waitlist_entries = waitlist_manager.get_user_waitlist_entries(guild_id, user_id).await?;
    assert_eq!(waitlist_entries.len(), 0); // Should be empty since entry was cancelled
    
    Ok(())
}

#[tokio::test]
async fn test_waitlist_offer_expiration() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let waitlist_manager = WaitlistManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let equipment_id = 1i64;
    let user_id = 100001i64;
    
    // Create test data
    sqlx::query!(
        "INSERT INTO guilds (id, offer_hold_minutes) VALUES (?, ?)",
        guild_id,
        1 // Very short hold time for testing
    )
    .execute(&ctx.db)
    .await?;
    
    sqlx::query!(
        "INSERT INTO equipment (id, guild_id, name, status) VALUES (?, ?, ?, ?)",
        equipment_id,
        guild_id,
        "Test Equipment",
        "Available"
    )
    .execute(&ctx.db)
    .await?;
    
    let now = Utc::now();
    let desired_start = now + Duration::hours(1);
    let desired_end = now + Duration::hours(3);
    
    // User joins waitlist
    waitlist_manager.join_waitlist(guild_id, equipment_id, user_id, desired_start, desired_end).await?;
    
    // Create offer
    let offer_result = waitlist_manager.create_offer_for_available_window(
        equipment_id,
        desired_start,
        desired_end,
        guild_id,
    ).await?;
    
    assert!(offer_result.is_some());
    let offer = offer_result.unwrap();
    
    // Manually expire the offer by updating the database
    sqlx::query!(
        "UPDATE waitlist_offers SET offer_expires_at_utc = ? WHERE id = ?",
        (now - Duration::minutes(1)).naive_utc(),
        offer.offer_id
    )
    .execute(&ctx.db)
    .await?;
    
    // Try to accept expired offer
    let acceptance_result = waitlist_manager.accept_offer(offer.offer_id, user_id).await?;
    assert!(acceptance_result.is_none()); // Should fail for expired offer
    
    // Process expired offers
    let expired_offers = waitlist_manager.process_expired_offers().await?;
    assert_eq!(expired_offers.len(), 1);
    assert_eq!(expired_offers[0], offer.offer_id);
    
    Ok(())
}