use anyhow::Result;
use oucc_kizai_bot::handlers::{Handler, TimeFilter, StatusFilter};
use oucc_kizai_bot::models::Reservation;
use sqlx::SqlitePool;

mod common;

// Basic test for management state and filtering logic
#[tokio::test]
async fn test_management_filter_logic() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Create test data
    let (guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;
    
    // Create a test reservation manually using SQL
    let now = chrono::Utc::now();
    let start_time = now + chrono::Duration::hours(1);
    let end_time = now + chrono::Duration::hours(3);
    
    let reservation_id = sqlx::query_scalar!(
        "INSERT INTO reservations (equipment_id, user_id, start_time, end_time, location, status, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
        equipment.id,
        12345i64,
        start_time,
        end_time,
        "Test Location",
        "Confirmed",
        now,
        now
    )
    .fetch_one(&ctx.db)
    .await?;

    let handler = Handler::new(ctx.db.clone());
    
    // Test basic reservation retrieval (this would normally be tested with mock Discord interactions)
    let reservations = handler.get_filtered_reservations(guild.id, &Default::default()).await?;
    
    assert!(!reservations.is_empty(), "Should find the created reservation");
    assert_eq!(reservations[0].id, reservation_id, "Should match created reservation ID");
    
    Ok(())
}

// Test filter functions
#[tokio::test]
async fn test_filter_functions() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let handler = Handler::new(ctx.db.clone());
    
    // Create test reservation
    let now = chrono::Utc::now();
    let reservation = Reservation {
        id: 1,
        equipment_id: 1,
        user_id: 12345,
        start_time: now + chrono::Duration::hours(1),
        end_time: now + chrono::Duration::hours(3),
        location: Some("Test Location".to_string()),
        status: "Confirmed".to_string(),
        created_at: now,
        updated_at: now,
        returned_at: None,
        return_location: None,
    };

    // Test equipment filter
    assert!(handler.matches_equipment_filter(&reservation, &None), "Should match when no filter");
    assert!(handler.matches_equipment_filter(&reservation, &Some(vec![1])), "Should match when equipment ID in filter");
    assert!(!handler.matches_equipment_filter(&reservation, &Some(vec![2])), "Should not match when equipment ID not in filter");

    // Test time filters
    assert!(handler.matches_time_filter(&reservation, &TimeFilter::All), "Should match All time filter");
    assert!(handler.matches_time_filter(&reservation, &TimeFilter::Next24h), "Should match Next24h filter for future reservation");
    
    // Test status filters
    assert!(handler.matches_status_filter(&reservation, &StatusFilter::All), "Should match All status filter");
    assert!(handler.matches_status_filter(&reservation, &StatusFilter::Upcoming), "Should match Upcoming for future reservation");
    assert!(!handler.matches_status_filter(&reservation, &StatusFilter::Active), "Should not match Active for future reservation");
    
    Ok(())
}

#[tokio::test]
async fn test_equipment_name_retrieval() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Create test equipment
    let (_guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;
    
    let handler = Handler::new(ctx.db.clone());
    
    // Test equipment name retrieval
    let name = handler.get_equipment_name(equipment.id).await?;
    assert_eq!(name, equipment.name, "Should return correct equipment name");
    
    // Test non-existent equipment
    let fallback_name = handler.get_equipment_name(99999).await?;
    assert!(fallback_name.starts_with("Equipment #"), "Should return fallback name for non-existent equipment");
    
    Ok(())
}

#[tokio::test]
async fn test_reservation_status_display() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let handler = Handler::new(ctx.db.clone());
    
    let now = chrono::Utc::now();
    
    // Test upcoming reservation
    let upcoming = Reservation {
        id: 1,
        equipment_id: 1,
        user_id: 12345,
        start_time: now + chrono::Duration::hours(1),
        end_time: now + chrono::Duration::hours(3),
        location: None,
        status: "Confirmed".to_string(),
        created_at: now,
        updated_at: now,
        returned_at: None,
        return_location: None,
    };
    
    let status = handler.get_reservation_display_status(&upcoming).await;
    assert_eq!(status, "Upcoming", "Should show Upcoming for future reservation");
    
    // Test returned reservation
    let returned = Reservation {
        id: 2,
        equipment_id: 1,
        user_id: 12345,
        start_time: now - chrono::Duration::hours(3),
        end_time: now - chrono::Duration::hours(1),
        location: None,
        status: "Confirmed".to_string(),
        created_at: now,
        updated_at: now,
        returned_at: Some(now - chrono::Duration::minutes(30)),
        return_location: None,
    };
    
    let status = handler.get_reservation_display_status(&returned).await;
    assert_eq!(status, "Returned", "Should show Returned when returned_at is set");
    
    Ok(())
}