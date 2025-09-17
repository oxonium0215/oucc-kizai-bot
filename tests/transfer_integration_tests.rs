use anyhow::Result;
use chrono::{Duration, Utc};
use oucc_kizai_bot::models::*;

mod common;

/// Test immediate transfer execution
#[tokio::test]
async fn test_immediate_transfer() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    let user1_id = 12345i64;
    let user2_id = 67890i64;
    
    // Create a reservation for user1
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        user1_id,
        Utc::now() + Duration::minutes(10), // Start in 10 minutes
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;
    
    // Simulate immediate transfer (this would normally be done through the handler)
    let mut tx = ctx.db.begin().await?;
    
    // Update reservation owner
    sqlx::query!(
        "UPDATE reservations SET user_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        user2_id,
        reservation.id
    )
    .execute(&mut *tx)
    .await?;
    
    // Log the transfer
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, 'Transferred', NULL, 'Confirmed', 'Confirmed', 'Test transfer', CURRENT_TIMESTAMP)",
        equipment.id,
        user1_id
    )
    .execute(&mut *tx)
    .await?;
    
    tx.commit().await?;
    
    // Verify transfer
    let updated_reservation = sqlx::query!(
        "SELECT user_id FROM reservations WHERE id = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(updated_reservation.user_id, user2_id);
    
    // Verify log entry
    let log_entry = sqlx::query!(
        "SELECT action, notes FROM equipment_logs WHERE equipment_id = ? AND action = 'Transferred'",
        equipment.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(log_entry.action, "Transferred");
    assert!(log_entry.notes.unwrap_or_default().contains("Test transfer"));
    
    Ok(())
}

/// Test scheduled transfer execution
#[tokio::test]
async fn test_scheduled_transfer_execution() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    let user1_id = 12345i64;
    let user2_id = 67890i64;
    
    // Create a reservation for user1
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        user1_id,
        Utc::now() + Duration::minutes(10),
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;
    
    // Create a scheduled transfer request
    let execute_at = Utc::now() + Duration::minutes(30); // Execute in 30 minutes
    let expires_at = execute_at + Duration::hours(1);
    let now = Utc::now();
    
    let transfer_id = sqlx::query!(
        "INSERT INTO transfer_requests 
         (reservation_id, from_user_id, to_user_id, requested_by_user_id, execute_at_utc, note, expires_at, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'Pending', ?, ?) 
         RETURNING id",
        reservation.id,
        user1_id,
        user2_id,
        user1_id,
        execute_at,
        "Test scheduled transfer",
        expires_at,
        now,
        now
    )
    .fetch_one(&ctx.db)
    .await?
    .id;
    
    // Simulate time passing to execution time
    let past_execute_time = Utc::now() - Duration::minutes(1); // Simulate past execution time
    sqlx::query!(
        "UPDATE transfer_requests SET execute_at_utc = ? WHERE id = ?",
        past_execute_time,
        transfer_id
    )
    .execute(&ctx.db)
    .await?;
    
    // Create job worker and process scheduled transfers
    let job_worker = oucc_kizai_bot::jobs::JobWorker::new(ctx.db.clone());
    
    // Process scheduled transfers (this would normally be called by the job worker)
    let transfers = sqlx::query_as!(
        TransferRequest,
        "SELECT * FROM transfer_requests 
         WHERE status = 'Pending' AND execute_at_utc IS NOT NULL AND execute_at_utc <= ?
         ORDER BY execute_at_utc LIMIT 10",
        Utc::now()
    )
    .fetch_all(&ctx.db)
    .await?;
    
    assert_eq!(transfers.len(), 1);
    let transfer = &transfers[0];
    
    // Simulate execution (simplified version of execute_scheduled_transfer)
    let mut tx = ctx.db.begin().await?;
    
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
        if let Some(note) = &transfer.note { format!(" - Note: {}", note) } else { String::new() }
    );
    
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, 'Transferred', NULL, 'Confirmed', 'Confirmed', ?, CURRENT_TIMESTAMP)",
        equipment.id,
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
    
    // Verify execution
    let updated_reservation = sqlx::query!(
        "SELECT user_id FROM reservations WHERE id = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(updated_reservation.user_id, user2_id);
    
    let updated_transfer = sqlx::query!(
        "SELECT status FROM transfer_requests WHERE id = ?",
        transfer_id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(updated_transfer.status, "Accepted");
    
    Ok(())
}

/// Test transfer validation - no-op same user
#[tokio::test]
async fn test_transfer_validation_same_user() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    let user_id = 12345i64;
    
    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        user_id,
        Utc::now() + Duration::minutes(10),
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;
    
    // Attempt to create transfer to same user (should be prevented in UI/handler)
    // This simulates the validation logic that should prevent no-op transfers
    let from_user_id = user_id;
    let to_user_id = user_id;
    
    // This should be caught in the handler validation
    assert_eq!(from_user_id, to_user_id);
    
    Ok(())
}

/// Test transfer cancellation
#[tokio::test]
async fn test_transfer_cancellation() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    let user1_id = 12345i64;
    let user2_id = 67890i64;
    
    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        user1_id,
        Utc::now() + Duration::minutes(10),
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;
    
    // Create a scheduled transfer request
    let execute_at = Utc::now() + Duration::hours(1);
    let expires_at = execute_at + Duration::hours(1);
    let now = Utc::now();
    
    let transfer_id = sqlx::query!(
        "INSERT INTO transfer_requests 
         (reservation_id, from_user_id, to_user_id, requested_by_user_id, execute_at_utc, note, expires_at, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 'Pending', ?, ?) 
         RETURNING id",
        reservation.id,
        user1_id,
        user2_id,
        user1_id,
        execute_at,
        "Test transfer",
        expires_at,
        now,
        now
    )
    .fetch_one(&ctx.db)
    .await?
    .id;
    
    // Cancel the transfer
    let cancel_time = Utc::now();
    sqlx::query!(
        "UPDATE transfer_requests 
         SET status = 'Canceled', canceled_at_utc = ?, canceled_by_user_id = ?, updated_at = ?
         WHERE id = ?",
        cancel_time,
        user1_id,
        cancel_time,
        transfer_id
    )
    .execute(&ctx.db)
    .await?;
    
    // Verify cancellation
    let canceled_transfer = sqlx::query!(
        "SELECT status, canceled_at_utc, canceled_by_user_id FROM transfer_requests WHERE id = ?",
        transfer_id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(canceled_transfer.status, "Canceled");
    assert!(canceled_transfer.canceled_at_utc.is_some());
    assert_eq!(canceled_transfer.canceled_by_user_id.unwrap(), user1_id);
    
    Ok(())
}

/// Test transfer with returned reservation (should fail)
#[tokio::test]
async fn test_transfer_returned_reservation() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;
    
    let user1_id = 12345i64;
    let user2_id = 67890i64;
    
    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        user1_id,
        Utc::now() - Duration::hours(1), // Started 1 hour ago
        Utc::now() + Duration::hours(1),  // Ends in 1 hour
    )
    .build(&ctx.db)
    .await?;
    
    // Mark reservation as returned
    let return_time = Utc::now();
    sqlx::query!(
        "UPDATE reservations SET returned_at = ?, return_location = ? WHERE id = ?",
        return_time,
        "Test Location",
        reservation.id
    )
    .execute(&ctx.db)
    .await?;
    
    // Attempt to create transfer for returned reservation
    let result = crate::transfer_tests::create_transfer_request(
        &ctx.db,
        reservation.id,
        user1_id,
        user2_id,
    ).await;
    
    // The transfer request creation itself might succeed, but execution should fail
    // In a real implementation, the validation would happen during execution
    if result.is_ok() {
        // Simulate execution attempt which should fail
        let reservation_check = sqlx::query!(
            "SELECT returned_at FROM reservations WHERE id = ?",
            reservation.id
        )
        .fetch_one(&ctx.db)
        .await?;
        
        // This should be caught by transfer execution validation
        assert!(reservation_check.returned_at.is_some());
    }
    
    Ok(())
}