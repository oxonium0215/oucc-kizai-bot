use anyhow::Result;
use chrono::Utc;
use oucc_kizai_bot::equipment::{EditAction, EquipmentRenderer};
use oucc_kizai_bot::models::{Equipment, ManagedMessage, Tag};

mod common;

/// Test the edit plan computation logic for different scenarios
#[tokio::test]
async fn test_edit_plan_computation() -> Result<()> {
    // Set up test data
    let equipment_list = vec![
        (
            Equipment {
                id: 1,
                guild_id: 123,
                tag_id: Some(1),
                name: "Camera A".to_string(),
                status: "Available".to_string(),
                current_location: None,
                unavailable_reason: None,
                default_return_location: Some("Storage Room".to_string()),
                message_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            Some(Tag {
                id: 1,
                guild_id: 123,
                name: "Cameras".to_string(),
                sort_order: 1,
                created_at: Utc::now(),
            }),
        ),
        (
            Equipment {
                id: 2,
                guild_id: 123,
                tag_id: Some(1),
                name: "Camera B".to_string(),
                status: "Loaned".to_string(),
                current_location: None,
                unavailable_reason: None,
                default_return_location: Some("Storage Room".to_string()),
                message_id: None,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            },
            Some(Tag {
                id: 1,
                guild_id: 123,
                name: "Cameras".to_string(),
                sort_order: 1,
                created_at: Utc::now(),
            }),
        ),
    ];

    // Test case 1: Empty existing messages - should create header + 2 equipment messages
    let existing_messages = vec![];
    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    assert_eq!(edit_plan.creates, 3); // 1 header + 2 equipment
    assert_eq!(edit_plan.edits, 0);
    assert_eq!(edit_plan.deletes, 0);

    // Test case 2: Header exists, no equipment messages - should create 2 equipment messages
    let existing_messages = vec![ManagedMessage {
        id: 1,
        guild_id: 123,
        channel_id: 456,
        message_id: 789,
        message_type: "Header".to_string(),
        equipment_id: None,
        sort_order: Some(0),
        created_at: Utc::now(),
    }];
    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    assert_eq!(edit_plan.creates, 2); // 2 equipment only
    assert_eq!(edit_plan.edits, 0);
    assert_eq!(edit_plan.deletes, 0);

    // Test case 3: All messages exist with correct equipment - should do nothing
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
            equipment_id: Some(1),
            sort_order: Some(1),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 3,
            guild_id: 123,
            channel_id: 456,
            message_id: 791,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(2),
            sort_order: Some(2),
            created_at: Utc::now(),
        },
    ];
    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    assert_eq!(edit_plan.creates, 0);
    assert_eq!(edit_plan.edits, 0);
    assert_eq!(edit_plan.deletes, 0);

    // Test case 4: Equipment changed - should edit existing messages
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
            equipment_id: Some(99), // Wrong equipment ID
            sort_order: Some(1),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 3,
            guild_id: 123,
            channel_id: 456,
            message_id: 791,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(2),
            sort_order: Some(2),
            created_at: Utc::now(),
        },
    ];
    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    assert_eq!(edit_plan.creates, 0);
    assert_eq!(edit_plan.edits, 1); // Should edit the first equipment message
    assert_eq!(edit_plan.deletes, 0);

    // Test case 5: Too many messages - should delete excess
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
            equipment_id: Some(1),
            sort_order: Some(1),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 3,
            guild_id: 123,
            channel_id: 456,
            message_id: 791,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(2),
            sort_order: Some(2),
            created_at: Utc::now(),
        },
        ManagedMessage {
            id: 4,
            guild_id: 123,
            channel_id: 456,
            message_id: 792,
            message_type: "EquipmentEmbed".to_string(),
            equipment_id: Some(3), // Extra message
            sort_order: Some(3),
            created_at: Utc::now(),
        },
    ];
    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    assert_eq!(edit_plan.creates, 0);
    assert_eq!(edit_plan.edits, 0);
    assert_eq!(edit_plan.deletes, 1); // Should delete the extra message

    println!("All edit plan computation tests passed!");
    Ok(())
}

/// Test the self-healing scenario where messages are out of order or missing
#[tokio::test]
async fn test_self_healing_scenarios() -> Result<()> {
    let equipment_list = vec![(
        Equipment {
            id: 1,
            guild_id: 123,
            tag_id: None,
            name: "Equipment A".to_string(),
            status: "Available".to_string(),
            current_location: None,
            unavailable_reason: None,
            default_return_location: None,
            message_id: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        },
        None,
    )];

    // Test case: Missing header - should create it
    let existing_messages = vec![ManagedMessage {
        id: 1,
        guild_id: 123,
        channel_id: 456,
        message_id: 790,
        message_type: "EquipmentEmbed".to_string(),
        equipment_id: Some(1),
        sort_order: Some(1),
        created_at: Utc::now(),
    }];

    let edit_plan = EquipmentRenderer::compute_edit_plan(&existing_messages, &equipment_list);

    assert_eq!(edit_plan.creates, 1); // Should create header
    assert_eq!(edit_plan.edits, 0);
    assert_eq!(edit_plan.deletes, 0);

    // Verify the action is CreateHeader
    assert!(edit_plan
        .actions
        .iter()
        .any(|action| matches!(action, EditAction::CreateHeader)));

    println!("Self-healing scenarios tests passed!");
    Ok(())
}
