use anyhow::Result;
use chrono::Utc;
use oucc_kizai_bot::equipment::EquipmentRenderer;
use oucc_kizai_bot::models::{Equipment, Tag, ManagedMessage};

mod common;

/// Integration test for the complete reservation channel rendering workflow
#[tokio::test]
async fn test_reconcile_integration() -> Result<()> {
    let db = common::setup_memory_db().await?;
    let renderer = EquipmentRenderer::new(db.clone());

    // Set up test data
    let guild_id = 123i64;
    let channel_id = 456i64;

    // Create a guild
    sqlx::query("INSERT INTO guilds (id, reservation_channel_id) VALUES (?, ?)")
        .bind(guild_id)
        .bind(channel_id)
        .execute(&db)
        .await?;

    // Create a tag
    sqlx::query(
        "INSERT INTO tags (id, guild_id, name, sort_order) VALUES (?, ?, ?, ?)"
    )
    .bind(1i64)
    .bind(guild_id)
    .bind("Test Tag")
    .bind(1i64)
    .execute(&db)
    .await?;

    // Create equipment
    sqlx::query(
        "INSERT INTO equipment (id, guild_id, tag_id, name, status) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(1i64)
    .bind(guild_id)
    .bind(1i64)
    .bind("Test Equipment")
    .bind("Available")
    .execute(&db)
    .await?;

    // Test 1: Verify get_ordered_equipment works
    let equipment_list = renderer.get_ordered_equipment(guild_id).await?;
    assert_eq!(equipment_list.len(), 1);
    assert_eq!(equipment_list[0].0.name, "Test Equipment");
    assert_eq!(equipment_list[0].1.as_ref().unwrap().name, "Test Tag");

    // Test 2: Verify edit plan computation for fresh start
    let existing_messages = vec![];
    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);
    
    // Should create header + 1 equipment message
    assert_eq!(edit_plan.creates, 2);
    assert_eq!(edit_plan.edits, 0);
    assert_eq!(edit_plan.deletes, 0);

    // Test 3: Simulate having existing messages and verify minimal updates
    let existing_messages = vec![
        ManagedMessage {
            id: 1,
            guild_id,
            channel_id,
            message_id: 789,
            message_type: "Header".to_string(),
            equipment_id: None,
            sort_order: Some(0),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 2,
            guild_id,
            channel_id,
            message_id: 790,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(1),
            sort_order: Some(1),
            created_at: Utc::now(),
        },
    ];

    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);
    
    // Should do nothing since everything matches
    assert_eq!(edit_plan.creates, 0);
    assert_eq!(edit_plan.edits, 0);  
    assert_eq!(edit_plan.deletes, 0);

    // Test 4: Verify equipment embed creation doesn't crash
    let embed = renderer.create_equipment_embed(&equipment_list[0].0, &equipment_list[0].1).await?;
    
    // Basic validation that embed was created
    assert!(embed.title.is_some());
    let title = embed.title.unwrap();
    assert!(title.contains("Test Equipment"));
    assert!(title.contains("✅")); // Available emoji

    println!("Integration test passed! All core functionality working.");
    Ok(())
}

/// Test the edit plan handles equipment reordering correctly  
#[tokio::test]
async fn test_equipment_reordering() -> Result<()> {
    // Test data with multiple equipment in different tag orders
    let equipment_list = vec![
        (Equipment {
            id: 2,
            guild_id: 123,
            tag_id: Some(2), 
            name: "Beta Equipment".to_string(),
            status: "Available".to_string(),
            current_location: None,
            unavailable_reason: None,
            default_return_location: None,
            message_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }, Some(Tag {
            id: 2,
            guild_id: 123,
            name: "Beta Tag".to_string(),
            sort_order: 1,  // Lower sort order = first
            created_at: Utc::now(),
        })),
        (Equipment {
            id: 1,
            guild_id: 123,
            tag_id: Some(1),
            name: "Alpha Equipment".to_string(),
            status: "Available".to_string(),
            current_location: None,
            unavailable_reason: None,
            default_return_location: None,
            message_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }, Some(Tag {
            id: 1,
            guild_id: 123,
            name: "Alpha Tag".to_string(),
            sort_order: 2,  // Higher sort order = second
            created_at: Utc::now(),
        })),
    ];

    // Existing messages in wrong order (Alpha first, Beta second)
    let existing_messages = vec![
        ManagedMessage {
            id: 1,
            guild_id: 123,
            channel_id: 456,
            message_id: 789,
            message_type: "Header".to_string(),
            equipment_id: None,
            sort_order: Some(0),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 2,
            guild_id: 123,
            channel_id: 456,
            message_id: 790,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(1), // Alpha equipment in position 1
            sort_order: Some(1),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 3,
            guild_id: 123,
            channel_id: 456,
            message_id: 791,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(2), // Beta equipment in position 2
            sort_order: Some(2),
            created_at: Utc::now(),
        },
    ];

    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    // Should edit both messages since equipment order changed
    // (Beta should be first now due to lower tag sort_order)
    assert_eq!(edit_plan.creates, 0);
    assert_eq!(edit_plan.edits, 2); // Both equipment messages need updating
    assert_eq!(edit_plan.deletes, 0);

    println!("Reordering test passed!");
    Ok(())
}