use anyhow::Result;
use oucc_kizai_bot::handlers::{Handler, LogTimeFilter, LogViewerState, StatusFilter, TimeFilter};
use oucc_kizai_bot::models::Reservation;

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
    let reservations = handler
        .get_filtered_reservations(guild.id, &Default::default())
        .await?;

    assert!(
        !reservations.is_empty(),
        "Should find the created reservation"
    );
    assert_eq!(
        reservations[0].id, reservation_id,
        "Should match created reservation ID"
    );

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
    assert!(
        handler.matches_equipment_filter(&reservation, &None),
        "Should match when no filter"
    );
    assert!(
        handler.matches_equipment_filter(&reservation, &Some(vec![1])),
        "Should match when equipment ID in filter"
    );
    assert!(
        !handler.matches_equipment_filter(&reservation, &Some(vec![2])),
        "Should not match when equipment ID not in filter"
    );

    // Test time filters
    assert!(
        handler.matches_time_filter(&reservation, &TimeFilter::All),
        "Should match All time filter"
    );
    assert!(
        handler.matches_time_filter(&reservation, &TimeFilter::Next24h),
        "Should match Next24h filter for future reservation"
    );

    // Test status filters
    assert!(
        handler.matches_status_filter(&reservation, &StatusFilter::All),
        "Should match All status filter"
    );
    assert!(
        handler.matches_status_filter(&reservation, &StatusFilter::Upcoming),
        "Should match Upcoming for future reservation"
    );
    assert!(
        !handler.matches_status_filter(&reservation, &StatusFilter::Active),
        "Should not match Active for future reservation"
    );

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
    assert!(
        fallback_name.starts_with("Equipment #"),
        "Should return fallback name for non-existent equipment"
    );

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
    assert_eq!(
        status, "Upcoming",
        "Should show Upcoming for future reservation"
    );

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
    assert_eq!(
        status, "Returned",
        "Should show Returned when returned_at is set"
    );

    Ok(())
}

// Test operation log functionality
#[tokio::test]
async fn test_operation_log_creation_and_retrieval() -> Result<()> {
    let ctx = common::TestContext::new().await?;

    // Create test data
    let (guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;

    let handler = Handler::new(ctx.db.clone());

    // Create test operation logs
    let now = chrono::Utc::now();
    let test_logs = vec![
        ("Reserved", Some("Test location"), Some("Equipment reserved by user")),
        ("Loaned", None, Some("Equipment loaned to user")), 
        ("Returned", Some("Return location"), Some("Equipment returned by user")),
    ];

    for (i, (action, location, notes)) in test_logs.iter().enumerate() {
        let timestamp = now - chrono::Duration::hours(24 - i as i64); // Spread across time
        
        sqlx::query!(
            "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
            equipment.id,
            12345i64,
            action,
            location,
            "Available",
            "Loaned",
            notes,
            timestamp
        )
        .execute(&ctx.db)
        .await?;
    }

    // Test log retrieval with different filters
    let default_state = LogViewerState::default();
    let logs = handler.get_filtered_operation_logs(guild.id, &default_state).await?;
    
    assert!(!logs.is_empty(), "Should find operation logs");
    assert!(logs.len() >= 3, "Should find at least the 3 test logs created");

    // Verify log content
    let reserved_log = logs.iter().find(|log| log.action == "Reserved");
    assert!(reserved_log.is_some(), "Should find Reserved log");
    
    let reserved_log = reserved_log.unwrap();
    assert_eq!(reserved_log.equipment_id, equipment.id, "Should match equipment ID");
    assert_eq!(reserved_log.user_id, 12345, "Should match user ID");
    assert_eq!(reserved_log.location, Some("Test location".to_string()), "Should match location");

    Ok(())
}

#[tokio::test]
async fn test_operation_log_time_filtering() -> Result<()> {
    let ctx = common::TestContext::new().await?;

    // Create test data
    let (guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;

    let handler = Handler::new(ctx.db.clone());

    let now = chrono::Utc::now();
    
    // Create logs with different timestamps
    let old_log_time = now - chrono::Duration::days(10);
    let recent_log_time = now - chrono::Duration::hours(2);
    let today_log_time = now - chrono::Duration::hours(1);

    // Old log (10 days ago)
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        equipment.id,
        12345i64,
        "OldAction",
        None::<&str>,
        "Available",
        "Loaned", 
        "Old log entry",
        old_log_time
    )
    .execute(&ctx.db)
    .await?;

    // Recent log (2 hours ago)
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        equipment.id,
        12345i64,
        "RecentAction",
        None::<&str>,
        "Available",
        "Loaned",
        "Recent log entry", 
        recent_log_time
    )
    .execute(&ctx.db)
    .await?;

    // Today's log (1 hour ago) 
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        equipment.id,
        12345i64,
        "TodayAction",
        None::<&str>,
        "Available",
        "Loaned",
        "Today's log entry",
        today_log_time
    )
    .execute(&ctx.db)
    .await?;

    // Test Today filter
    let today_state = LogViewerState {
        time_filter: LogTimeFilter::Today,
        equipment_filter: None,
        action_filter: None,
        page: 0,
        items_per_page: 15,
    };
    
    let today_logs = handler.get_filtered_operation_logs(guild.id, &today_state).await?;
    let today_actions: Vec<&str> = today_logs.iter().map(|log| log.action.as_str()).collect();
    assert!(today_actions.contains(&"TodayAction"), "Should include today's log");
    // Note: Recent log might also be included if it's within the same day

    // Test Last7Days filter  
    let week_state = LogViewerState {
        time_filter: LogTimeFilter::Last7Days,
        equipment_filter: None,
        action_filter: None,
        page: 0,
        items_per_page: 15,
    };
    
    let week_logs = handler.get_filtered_operation_logs(guild.id, &week_state).await?;
    let week_actions: Vec<&str> = week_logs.iter().map(|log| log.action.as_str()).collect();
    assert!(week_actions.contains(&"TodayAction"), "Should include today's log in week filter");
    assert!(week_actions.contains(&"RecentAction"), "Should include recent log in week filter");
    assert!(!week_actions.contains(&"OldAction"), "Should not include old log in week filter");

    // Test All filter
    let all_state = LogViewerState {
        time_filter: LogTimeFilter::All,
        equipment_filter: None,
        action_filter: None,
        page: 0,
        items_per_page: 15,
    };
    
    let all_logs = handler.get_filtered_operation_logs(guild.id, &all_state).await?;
    let all_actions: Vec<&str> = all_logs.iter().map(|log| log.action.as_str()).collect();
    assert!(all_actions.contains(&"TodayAction"), "Should include today's log in all filter");
    assert!(all_actions.contains(&"RecentAction"), "Should include recent log in all filter");
    assert!(all_actions.contains(&"OldAction"), "Should include old log in all filter");

    Ok(())
}

#[tokio::test]
async fn test_operation_log_equipment_filtering() -> Result<()> {
    let ctx = common::TestContext::new().await?;

    // Create test data with multiple equipment
    let (guild, tag, _location, equipment1) = common::create_test_setup(&ctx).await?;
    
    // Create second equipment
    let now = chrono::Utc::now();
    let equipment2_id = sqlx::query_scalar!(
        "INSERT INTO equipment (guild_id, tag_id, name, status, created_at, updated_at) 
         VALUES (?, ?, ?, ?, ?, ?) RETURNING id",
        guild.id,
        tag.id,
        "Test Equipment 2",
        "Available",
        now,
        now
    )
    .fetch_one(&ctx.db)
    .await?;

    let handler = Handler::new(ctx.db.clone());

    let now = chrono::Utc::now();

    // Create logs for both equipment
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        equipment1.id,
        12345i64,
        "Equipment1Action",
        None::<&str>,
        "Available",
        "Loaned",
        "Log for equipment 1",
        now
    )
    .execute(&ctx.db)
    .await?;

    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        equipment2_id,
        12345i64,
        "Equipment2Action",
        None::<&str>,
        "Available",
        "Loaned",
        "Log for equipment 2",
        now
    )
    .execute(&ctx.db)
    .await?;

    // Test equipment filtering
    let filtered_state = LogViewerState {
        time_filter: LogTimeFilter::All,
        equipment_filter: Some(vec![equipment1.id]),
        action_filter: None,
        page: 0,
        items_per_page: 15,
    };
    
    let filtered_logs = handler.get_filtered_operation_logs(guild.id, &filtered_state).await?;
    
    // Should only contain logs for equipment1
    for log in &filtered_logs {
        assert_eq!(log.equipment_id, equipment1.id, "Should only include logs for filtered equipment");
    }
    
    let actions: Vec<&str> = filtered_logs.iter().map(|log| log.action.as_str()).collect();
    assert!(actions.contains(&"Equipment1Action"), "Should include equipment 1 action");
    assert!(!actions.contains(&"Equipment2Action"), "Should not include equipment 2 action");

    Ok(())
}
