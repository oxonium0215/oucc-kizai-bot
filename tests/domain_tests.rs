use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use oucc_kizai_bot::models::*;
use sqlx::Executor;

mod common;

/// Check if two reservations overlap
pub fn reservations_overlap(
    start1: DateTime<Utc>,
    end1: DateTime<Utc>,
    start2: DateTime<Utc>,
    end2: DateTime<Utc>,
) -> bool {
    // Two reservations overlap if one starts before the other ends
    // and vice versa (excluding exact boundaries)
    start1 < end2 && start2 < end1
}

/// Check for reservation conflicts in database
pub async fn check_reservation_conflict<'e, E>(
    db: E,
    equipment_id: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    exclude_reservation_id: Option<i64>,
) -> Result<bool>
where
    E: Executor<'e, Database = sqlx::Sqlite>,
{
    let count = if let Some(exclude_id) = exclude_reservation_id {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' 
             AND id != ? AND start_time < ? AND end_time > ?",
            equipment_id,
            exclude_id,
            end_time,
            start_time
        )
        .fetch_one(db)
        .await?
    } else {
        sqlx::query_scalar!(
            "SELECT COUNT(*) FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' 
             AND start_time < ? AND end_time > ?",
            equipment_id,
            end_time,
            start_time
        )
        .fetch_one(db)
        .await?
    };

    Ok(count > 0)
}

/// Calculate return correction window
pub fn is_within_return_correction_window(
    return_time: DateTime<Utc>,
    next_reservation_start: Option<DateTime<Utc>>,
    current_time: DateTime<Utc>,
) -> bool {
    // Rule: allowed until earlier of 1h after return or 15m before next reservation
    let one_hour_after_return = return_time + Duration::hours(1);

    let deadline = if let Some(next_start) = next_reservation_start {
        let fifteen_min_before_next = next_start - Duration::minutes(15);
        std::cmp::min(one_hour_after_return, fifteen_min_before_next)
    } else {
        one_hour_after_return
    };

    current_time <= deadline
}

/// Equipment sorting comparator
pub fn sort_equipment_key(equipment: &Equipment, tag_sort_order: Option<i64>) -> (i64, String) {
    (tag_sort_order.unwrap_or(i64::MAX), equipment.name.clone())
}

#[tokio::test]
async fn test_reservation_overlap_detection() {
    // Test various overlap scenarios
    let base_time = Utc::now();
    let hour = Duration::hours(1);

    // Case 1: No overlap - adjacent reservations
    let start1 = base_time;
    let end1 = base_time + hour;
    let start2 = base_time + hour;
    let end2 = base_time + hour * 2;
    assert!(!reservations_overlap(start1, end1, start2, end2));

    // Case 2: Complete overlap - one inside another
    let start1 = base_time;
    let end1 = base_time + hour * 3;
    let start2 = base_time + hour;
    let end2 = base_time + hour * 2;
    assert!(reservations_overlap(start1, end1, start2, end2));

    // Case 3: Partial overlap - crossing reservations
    let start1 = base_time;
    let end1 = base_time + hour * 2;
    let start2 = base_time + hour;
    let end2 = base_time + hour * 3;
    assert!(reservations_overlap(start1, end1, start2, end2));

    // Case 4: Same exact times
    let start1 = base_time;
    let end1 = base_time + hour;
    let start2 = base_time;
    let end2 = base_time + hour;
    assert!(reservations_overlap(start1, end1, start2, end2));

    // Case 5: Cross-day overlap
    let start1 = base_time;
    let end1 = base_time + Duration::hours(25); // Next day
    let start2 = base_time + Duration::hours(12); // Same day
    let end2 = base_time + Duration::hours(13);
    assert!(reservations_overlap(start1, end1, start2, end2));
}

#[tokio::test]
async fn test_reservation_conflict_check() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;

    let base_time = Utc::now();
    let hour = Duration::hours(1);

    // Create a confirmed reservation
    let reservation1 =
        common::ReservationBuilder::new(equipment.id, 12345, base_time, base_time + hour * 2)
            .build(&ctx.db)
            .await?;

    // Test conflict detection for overlapping times
    let has_conflict = check_reservation_conflict(
        &ctx.db,
        equipment.id,
        base_time + hour, // Overlaps with existing reservation
        base_time + hour * 3,
        None,
    )
    .await?;
    assert!(has_conflict);

    // Test no conflict for adjacent times
    let has_conflict = check_reservation_conflict(
        &ctx.db,
        equipment.id,
        base_time + hour * 2, // Starts when previous ends
        base_time + hour * 3,
        None,
    )
    .await?;
    assert!(!has_conflict);

    // Test excluding a reservation from conflict check
    let has_conflict = check_reservation_conflict(
        &ctx.db,
        equipment.id,
        base_time,
        base_time + hour * 2,
        Some(reservation1.id),
    )
    .await?;
    assert!(!has_conflict); // Should not conflict with itself

    Ok(())
}

#[tokio::test]
async fn test_concurrent_reservation_attempts() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;

    let base_time = Utc::now();
    let hour = Duration::hours(1);

    // Simulate concurrent reservation attempts
    let user1_id = 11111i64;
    let user2_id = 22222i64;
    let start_time = base_time + hour;
    let end_time = base_time + hour * 2;

    // Create two connections to simulate concurrent access
    let db1 = ctx.db.clone();
    let db2 = ctx.db.clone();

    // Both users try to reserve the same time slot simultaneously
    let task1 = tokio::spawn(async move {
        let mut tx = db1.begin().await.unwrap();

        // Check for conflicts
        let has_conflict =
            check_reservation_conflict(&mut *tx, equipment.id, start_time, end_time, None)
                .await
                .unwrap();

        if !has_conflict {
            // Insert reservation
            sqlx::query!(
                "INSERT INTO reservations (equipment_id, user_id, start_time, end_time, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, 'Confirmed', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                equipment.id,
                user1_id,
                start_time,
                end_time
            )
            .execute(&mut *tx)
            .await.unwrap();

            // Small delay to simulate processing time
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            tx.commit().await.unwrap();
            true
        } else {
            tx.rollback().await.unwrap();
            false
        }
    });

    let task2 = tokio::spawn(async move {
        let mut tx = db2.begin().await.unwrap();

        // Check for conflicts
        let has_conflict =
            check_reservation_conflict(&mut *tx, equipment.id, start_time, end_time, None)
                .await
                .unwrap();

        if !has_conflict {
            // Insert reservation
            sqlx::query!(
                "INSERT INTO reservations (equipment_id, user_id, start_time, end_time, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, 'Confirmed', CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                equipment.id,
                user2_id,
                start_time,
                end_time
            )
            .execute(&mut *tx)
            .await.unwrap();

            // Small delay to simulate processing time
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            tx.commit().await.unwrap();
            true
        } else {
            tx.rollback().await.unwrap();
            false
        }
    });

    let (result1, result2) = tokio::join!(task1, task2);
    let success1 = result1.unwrap();
    let success2 = result2.unwrap();

    // Only one should succeed due to foreign key constraints and transaction isolation
    assert!(
        success1 ^ success2,
        "Exactly one reservation should succeed"
    );

    // Verify only one reservation exists
    let count = sqlx::query!(
        "SELECT COUNT(*) as count FROM reservations WHERE equipment_id = ? AND start_time = ?",
        equipment.id,
        start_time
    )
    .fetch_one(&ctx.db)
    .await?;

    assert_eq!(count.count, 1);

    Ok(())
}

#[tokio::test]
async fn test_return_correction_window() {
    let base_time = Utc::now();
    let hour = Duration::hours(1);
    let _minute = Duration::minutes(1);

    let return_time = base_time;

    // Case 1: Within 1 hour, no next reservation
    let current_time = return_time + Duration::minutes(30);
    assert!(is_within_return_correction_window(
        return_time,
        None,
        current_time
    ));

    let current_time = return_time + Duration::minutes(61);
    assert!(!is_within_return_correction_window(
        return_time,
        None,
        current_time
    ));

    // Case 2: With next reservation - 15 minutes before next is the limit, but 1h window still applies
    let next_reservation = return_time + hour * 2; // 2 hours later
    let current_time = next_reservation - Duration::minutes(10); // Too close to next (10 min before)
    assert!(!is_within_return_correction_window(
        return_time,
        Some(next_reservation),
        current_time
    ));

    // This should fail because we're past the 1-hour window (100 min after return)
    let current_time = next_reservation - Duration::minutes(20); // 20 min before next = 100 min after return
    assert!(!is_within_return_correction_window(
        return_time,
        Some(next_reservation),
        current_time
    ));

    // This should pass because we're within 1 hour and safe from next reservation
    let current_time = return_time + Duration::minutes(50); // 50 min after return, 70 min before next
    assert!(is_within_return_correction_window(
        return_time,
        Some(next_reservation),
        current_time
    ));

    // Case 3: Next reservation is very close (less than 1 hour)
    let next_reservation = return_time + Duration::minutes(30);
    let current_time = return_time + Duration::minutes(10);
    assert!(is_within_return_correction_window(
        return_time,
        Some(next_reservation),
        current_time
    ));

    let current_time = return_time + Duration::minutes(20); // 10 minutes before next
    assert!(!is_within_return_correction_window(
        return_time,
        Some(next_reservation),
        current_time
    ));
}

#[test]
fn test_equipment_sorting() {
    // Create test equipment with different tags and names
    let equipment1 = Equipment {
        id: 1,
        guild_id: 1,
        tag_id: Some(1),
        name: "Zebra Camera".to_string(),
        status: "Available".to_string(),
        current_location: None,
        unavailable_reason: None,
        default_return_location: None,
        message_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let equipment2 = Equipment {
        id: 2,
        guild_id: 1,
        tag_id: Some(2),
        name: "Alpha Camera".to_string(),
        status: "Available".to_string(),
        current_location: None,
        unavailable_reason: None,
        default_return_location: None,
        message_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let equipment3 = Equipment {
        id: 3,
        guild_id: 1,
        tag_id: Some(1),
        name: "Alpha Lens".to_string(),
        status: "Available".to_string(),
        current_location: None,
        unavailable_reason: None,
        default_return_location: None,
        message_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Create a list with equipment and their tag sort orders
    let mut equipment_with_tags = vec![
        (&equipment1, Some(5i64)), // Tag order 5, name "Zebra Camera"
        (&equipment2, Some(1i64)), // Tag order 1, name "Alpha Camera"
        (&equipment3, Some(5i64)), // Tag order 5, name "Alpha Lens"
    ];

    // Sort by tag order first, then by name
    equipment_with_tags.sort_by_key(|(eq, tag_order)| sort_equipment_key(eq, *tag_order));

    // Should be: Alpha Camera (tag 1), Alpha Lens (tag 5), Zebra Camera (tag 5)
    assert_eq!(equipment_with_tags[0].0.name, "Alpha Camera");
    assert_eq!(equipment_with_tags[1].0.name, "Alpha Lens");
    assert_eq!(equipment_with_tags[2].0.name, "Zebra Camera");
}

#[test]
fn test_equipment_sorting_no_tags() {
    let equipment1 = Equipment {
        id: 1,
        guild_id: 1,
        tag_id: None,
        name: "Zebra Camera".to_string(),
        status: "Available".to_string(),
        current_location: None,
        unavailable_reason: None,
        default_return_location: None,
        message_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let equipment2 = Equipment {
        id: 2,
        guild_id: 1,
        tag_id: Some(1),
        name: "Alpha Camera".to_string(),
        status: "Available".to_string(),
        current_location: None,
        unavailable_reason: None,
        default_return_location: None,
        message_id: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    let mut equipment_with_tags = vec![
        (&equipment1, None),       // No tag (should sort last)
        (&equipment2, Some(1i64)), // Tag order 1
    ];

    equipment_with_tags.sort_by_key(|(eq, tag_order)| sort_equipment_key(eq, *tag_order));

    // Tagged equipment should come first
    assert_eq!(equipment_with_tags[0].0.name, "Alpha Camera");
    assert_eq!(equipment_with_tags[1].0.name, "Zebra Camera");
}

#[tokio::test]
async fn test_maintenance_window_conflict_detection() -> Result<()> {
    let ctx = common::setup_test_db().await?;

    // Create test equipment
    let equipment_id = sqlx::query!(
        "INSERT INTO equipment (guild_id, name, status) VALUES (1, 'Test Equipment', 'Available')"
    )
    .execute(&ctx.db)
    .await?
    .last_insert_rowid();

    let base_time = Utc::now();
    let hour = Duration::hours(1);

    // Create a maintenance window
    let maintenance_start = base_time + hour;
    let maintenance_end = base_time + hour * 3;

    sqlx::query!(
        "INSERT INTO maintenance_windows (equipment_id, start_utc, end_utc, reason, created_by_user_id) 
         VALUES (?, ?, ?, 'Test maintenance', 123)",
        equipment_id,
        maintenance_start.naive_utc(),
        maintenance_end.naive_utc()
    )
    .execute(&ctx.db)
    .await?;

    // Test that we can detect overlap with maintenance windows
    let maintenance_helper = oucc_kizai_bot::maintenance::MaintenanceHelper::new(ctx.db.clone());

    // Test 1: Overlapping reservation should conflict
    let conflict_start = base_time + Duration::minutes(30);
    let conflict_end = base_time + hour * 2;

    let conflict = maintenance_helper
        .check_maintenance_conflict(equipment_id, conflict_start, conflict_end)
        .await?;

    assert!(
        conflict.is_some(),
        "Should detect conflict with maintenance window"
    );

    // Test 2: Non-overlapping reservation should not conflict
    let no_conflict_start = base_time + hour * 4;
    let no_conflict_end = base_time + hour * 5;

    let no_conflict = maintenance_helper
        .check_maintenance_conflict(equipment_id, no_conflict_start, no_conflict_end)
        .await?;

    assert!(
        no_conflict.is_none(),
        "Should not detect conflict outside maintenance window"
    );

    // Test 3: Exactly adjacent times should not conflict
    let adjacent_start = base_time + hour * 3; // Starts when maintenance ends
    let adjacent_end = base_time + hour * 4;

    let adjacent_conflict = maintenance_helper
        .check_maintenance_conflict(equipment_id, adjacent_start, adjacent_end)
        .await?;

    assert!(
        adjacent_conflict.is_none(),
        "Should not conflict with adjacent times"
    );

    Ok(())
}

#[tokio::test]
async fn test_maintenance_window_creation_with_overlap_check() -> Result<()> {
    let ctx = common::setup_test_db().await?;

    // Create test equipment
    let equipment_id = sqlx::query!(
        "INSERT INTO equipment (guild_id, name, status) VALUES (1, 'Test Equipment', 'Available')"
    )
    .execute(&ctx.db)
    .await?
    .last_insert_rowid();

    let base_time = Utc::now();
    let hour = Duration::hours(1);

    let maintenance_helper = oucc_kizai_bot::maintenance::MaintenanceHelper::new(ctx.db.clone());

    // Test 1: Create first maintenance window
    let start1 = base_time + hour;
    let end1 = base_time + hour * 3;

    let result1 = maintenance_helper
        .create_maintenance_window(
            equipment_id,
            start1,
            end1,
            Some("First maintenance".to_string()),
            123,
        )
        .await;

    assert!(
        result1.is_ok(),
        "First maintenance window should be created successfully"
    );

    // Test 2: Try to create overlapping maintenance window (should fail)
    let start2 = base_time + hour * 2; // Overlaps with first window
    let end2 = base_time + hour * 4;

    let result2 = maintenance_helper
        .create_maintenance_window(
            equipment_id,
            start2,
            end2,
            Some("Second maintenance".to_string()),
            123,
        )
        .await;

    assert!(
        result2.is_err(),
        "Overlapping maintenance window should be rejected"
    );
    assert!(
        result2.unwrap_err().contains("overlap"),
        "Error should mention overlap"
    );

    // Test 3: Create non-overlapping maintenance window (should succeed)
    let start3 = base_time + hour * 5;
    let end3 = base_time + hour * 7;

    let result3 = maintenance_helper
        .create_maintenance_window(
            equipment_id,
            start3,
            end3,
            Some("Third maintenance".to_string()),
            123,
        )
        .await;

    assert!(
        result3.is_ok(),
        "Non-overlapping maintenance window should be created successfully"
    );

    Ok(())
}
