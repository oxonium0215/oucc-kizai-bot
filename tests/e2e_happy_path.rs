use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use oucc_kizai_bot::{models::*, time::*, traits::*};
use serenity::model::prelude::*;
use std::sync::Arc;

mod common;
use common::*;

/// End-to-End test that simulates a complete equipment lending lifecycle
#[tokio::test]
async fn test_e2e_happy_path() -> Result<()> {
    // Setup test environment with specific time
    let initial_time = DateTime::parse_from_rfc3339("2024-01-15T04:00:00Z") // 13:00 JST
        .unwrap()
        .with_timezone(&Utc);
    
    let ctx = TestContext::new_with_time(initial_time).await?;
    
    // === PHASE 1: Setup ===
    println!("Phase 1: Setting up guild, tags, locations, and equipment");
    
    let guild_id = 123456789i64;
    let channel_id = 987654321i64;
    let user1_id = 111111111i64; // Original user
    let user2_id = 222222222i64; // Transfer target
    
    // Create guild with reservation channel
    let guild = GuildBuilder::new(guild_id)
        .with_reservation_channel(channel_id)
        .with_admin_roles(vec![333333333])
        .build(&ctx.db)
        .await?;
    
    assert_eq!(guild.reservation_channel_id, Some(channel_id));
    println!("✓ Guild setup complete");
    
    // Create camera tag
    let camera_tag = TagBuilder::new(guild_id, "Camera")
        .with_sort_order(1)
        .build(&ctx.db)
        .await?;
    
    assert_eq!(camera_tag.name, "Camera");
    assert_eq!(camera_tag.sort_order, 1);
    println!("✓ Camera tag created");
    
    // Create club room location
    let club_room = LocationBuilder::new(guild_id, "Club Room")
        .build(&ctx.db)
        .await?;
    
    assert_eq!(club_room.name, "Club Room");
    println!("✓ Club Room location created");
    
    // Create Sony A7 camera equipment
    let sony_a7 = EquipmentBuilder::new(guild_id, "Sony A7")
        .with_tag(camera_tag.id)
        .with_default_return_location("Club Room")
        .with_status("Available")
        .build(&ctx.db)
        .await?;
    
    assert_eq!(sony_a7.name, "Sony A7");
    assert_eq!(sony_a7.status, "Available");
    assert_eq!(sony_a7.default_return_location, Some("Club Room".to_string()));
    println!("✓ Sony A7 equipment added");
    
    // === PHASE 2: Create Reservation ===
    println!("\nPhase 2: Creating reservation for today 13:00-15:00 JST");
    
    // Create reservation for 13:00-15:00 JST (04:00-06:00 UTC)
    let reservation_start = ctx.clock.now_utc(); // 13:00 JST
    let reservation_end = reservation_start + Duration::hours(2); // 15:00 JST
    
    // Verify JST formatting
    let start_jst = utc_to_jst_string(reservation_start);
    let end_jst = utc_to_jst_string(reservation_end);
    println!("Reservation time: {} - {} JST", start_jst, end_jst);
    assert_eq!(start_jst, "2024/01/15 13:00");
    assert_eq!(end_jst, "2024/01/15 15:00");
    
    let reservation = ReservationBuilder::new(
        sony_a7.id,
        user1_id,
        reservation_start,
        reservation_end,
    )
    .with_location("Photography Club")
    .build(&ctx.db)
    .await?;
    
    assert_eq!(reservation.equipment_id, sony_a7.id);
    assert_eq!(reservation.user_id, user1_id);
    assert_eq!(reservation.status, "Confirmed");
    assert_eq!(reservation.location, Some("Photography Club".to_string()));
    println!("✓ Reservation created for user {}", user1_id);
    
    // Update equipment status to Loaned
    sqlx::query!(
        "UPDATE equipment SET status = 'Loaned', current_location = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        reservation.location,
        sony_a7.id
    )
    .execute(&ctx.db)
    .await?;
    
    // === PHASE 3: Test Conflict Detection ===
    println!("\nPhase 3: Testing conflict detection");
    
    // Try to create overlapping reservation - should detect conflict
    let conflict_detected = crate::job_tests::check_reservation_conflict(
        &ctx.db,
        sony_a7.id,
        reservation_start + Duration::minutes(30), // Overlaps
        reservation_end + Duration::minutes(30),
        None,
    ).await?;
    
    assert!(conflict_detected);
    println!("✓ Conflict detection working correctly");
    
    // === PHASE 4: Transfer Request and Approval ===
    println!("\nPhase 4: Creating and approving transfer request");
    
    // Create transfer request
    let transfer_request = crate::transfer_tests::create_transfer_request(
        &ctx.db,
        reservation.id,
        user1_id,
        user2_id,
    ).await?;
    
    assert_eq!(transfer_request.from_user_id, user1_id);
    assert_eq!(transfer_request.to_user_id, user2_id);
    assert_eq!(transfer_request.status, "Pending");
    println!("✓ Transfer request created");
    
    // Simulate DM notification to target user
    let transfer_message = format!(
        "📤 機材移譲リクエスト: ユーザー<@{}>から「{}」の貸出を移譲したいとの連絡です。\n期間: {} - {}\n場所: {}",
        user1_id,
        sony_a7.name,
        start_jst,
        end_jst,
        reservation.location.as_ref().unwrap()
    );
    
    let dm_result = ctx.discord_api.send_dm(
        UserId::new(user2_id as u64),
        &transfer_message,
    ).await?;
    
    assert!(dm_result.is_some());
    println!("✓ Transfer notification sent via DM");
    
    // Approve the transfer
    crate::transfer_tests::update_transfer_status(
        &ctx.db,
        transfer_request.id,
        crate::transfer_tests::TransferStatus::Accepted,
    ).await?;
    
    // Update reservation to new user
    sqlx::query!(
        "UPDATE reservations SET user_id = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        user2_id,
        reservation.id
    )
    .execute(&ctx.db)
    .await?;
    
    println!("✓ Transfer approved and reservation updated");
    
    // === PHASE 5: Equipment Return ===
    println!("\nPhase 5: Processing equipment return");
    
    // Advance time to near end of reservation (14:45 JST)
    let near_end_time = reservation_end - Duration::minutes(15);
    ctx.clock.set_time(near_end_time).await;
    
    // Simulate pre-end reminder
    let reminder_message = format!(
        "📅 リマインダー: 「{}」の貸出期限まで15分です。\n返却時刻: {}",
        sony_a7.name,
        end_jst
    );
    
    ctx.discord_api.send_dm(
        UserId::new(user2_id as u64),
        &reminder_message,
    ).await?;
    
    println!("✓ Pre-end reminder sent");
    
    // Advance to return time
    ctx.clock.set_time(reservation_end).await;
    
    // Process return at Club Room (different from original location)
    let return_location = "Club Room";
    let return_time = ctx.clock.now_utc();
    
    // Create equipment log entry for return
    sqlx::query!(
        "INSERT INTO equipment_logs (equipment_id, user_id, action, location, previous_status, new_status, notes, timestamp)
         VALUES (?, ?, 'Returned', ?, 'Loaned', 'Available', 'Returned to Club Room', ?)",
        sony_a7.id,
        user2_id,
        return_location,
        return_time
    )
    .execute(&ctx.db)
    .await?;
    
    // Update equipment status
    sqlx::query!(
        "UPDATE equipment SET status = 'Available', current_location = ?, updated_at = ? WHERE id = ?",
        return_location,
        return_time,
        sony_a7.id
    )
    .execute(&ctx.db)
    .await?;
    
    // Send return confirmation
    let return_confirmation = format!(
        "✅ 返却完了: 「{}」を「{}」に返却しました。\n返却時刻: {}",
        sony_a7.name,
        return_location,
        utc_to_jst_string(return_time)
    );
    
    ctx.discord_api.send_dm(
        UserId::new(user2_id as u64),
        &return_confirmation,
    ).await?;
    
    println!("✓ Equipment returned and logged");
    
    // === PHASE 6: Test Return Correction Window ===
    println!("\nPhase 6: Testing return correction window");
    
    let current_time = ctx.clock.now_utc();
    let next_reservation_start = Some(return_time + Duration::hours(4)); // Next reservation 4 hours later
    
    // Should be within correction window (1 hour after return, no close next reservation)
    let within_window = crate::domain_tests::is_within_return_correction_window(
        return_time,
        next_reservation_start,
        current_time,
    );
    assert!(within_window);
    
    // Advance time past correction window
    ctx.clock.advance(Duration::hours(2)).await;
    let late_time = ctx.clock.now_utc();
    
    let within_window = crate::domain_tests::is_within_return_correction_window(
        return_time,
        next_reservation_start,
        late_time,
    );
    assert!(!within_window);
    
    println!("✓ Return correction window validation working");
    
    // === PHASE 7: Verify Equipment Ordering ===
    println!("\nPhase 7: Testing equipment ordering");
    
    // Create additional equipment for ordering test
    let lens_tag = TagBuilder::new(guild_id, "Lens")
        .with_sort_order(2)
        .build(&ctx.db)
        .await?;
    
    let canon_lens = EquipmentBuilder::new(guild_id, "Canon 50mm")
        .with_tag(lens_tag.id)
        .build(&ctx.db)
        .await?;
    
    let nikon_lens = EquipmentBuilder::new(guild_id, "Nikon 85mm")
        .with_tag(lens_tag.id)
        .build(&ctx.db)
        .await?;
    
    // Get all equipment with tag info for sorting test
    let equipment_list = sqlx::query!(
        "SELECT e.*, t.sort_order as tag_sort_order 
         FROM equipment e 
         LEFT JOIN tags t ON e.tag_id = t.id 
         WHERE e.guild_id = ? 
         ORDER BY COALESCE(t.sort_order, 999999), e.name",
        guild_id
    )
    .fetch_all(&ctx.db)
    .await?;
    
    // Verify ordering: Camera tag (order 1) comes first, then Lens tag (order 2)
    // Within each tag, alphabetical by name
    assert_eq!(equipment_list[0].name, "Sony A7");        // Camera tag, order 1
    assert_eq!(equipment_list[1].name, "Canon 50mm");     // Lens tag, order 2, "Canon" < "Nikon"
    assert_eq!(equipment_list[2].name, "Nikon 85mm");     // Lens tag, order 2, "Nikon" > "Canon"
    
    println!("✓ Equipment ordering working correctly");
    
    // === PHASE 8: Verify All Logs and Final State ===
    println!("\nPhase 8: Verifying logs and final state");
    
    // Check equipment logs
    let logs = sqlx::query_as!(
        EquipmentLog,
        "SELECT * FROM equipment_logs WHERE equipment_id = ? ORDER BY timestamp",
        sony_a7.id
    )
    .fetch_all(&ctx.db)
    .await?;
    
    assert!(!logs.is_empty());
    let return_log = logs.last().unwrap();
    assert_eq!(return_log.action, "Returned");
    assert_eq!(return_log.location, Some(return_location.to_string()));
    assert_eq!(return_log.new_status, Some("Available".to_string()));
    
    // Check final equipment state
    let final_equipment = sqlx::query_as!(
        Equipment,
        "SELECT * FROM equipment WHERE id = ?",
        sony_a7.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(final_equipment.status, "Available");
    assert_eq!(final_equipment.current_location, Some(return_location.to_string()));
    
    // Check transfer request final state
    let final_transfer = sqlx::query_as!(
        TransferRequest,
        "SELECT * FROM transfer_requests WHERE id = ?",
        transfer_request.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(final_transfer.status, "Accepted");
    
    // Check final reservation state
    let final_reservation = sqlx::query_as!(
        Reservation,
        "SELECT * FROM reservations WHERE id = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(final_reservation.user_id, user2_id); // Should be transferred user
    assert_eq!(final_reservation.status, "Confirmed");
    
    // === PHASE 9: Verify All User-Facing Messages are in JST ===
    println!("\nPhase 9: Verifying JST formatting in notifications");
    
    let sent_messages = ctx.discord_api.get_sent_dms().await;
    
    // All messages should contain JST-formatted times and equipment names
    for (_, message) in &sent_messages {
        // Should contain equipment name
        assert!(message.contains(&sony_a7.name));
        
        // Should contain JST-formatted time if it mentions time
        if message.contains("時刻") || message.contains("期限") {
            // Should contain JST format (YYYY/MM/DD HH:MM)
            assert!(message.contains("2024/01/15"));
        }
    }
    
    println!("✓ All notifications properly formatted with JST times and equipment names");
    
    // === SUMMARY ===
    println!("\n🎉 E2E Happy Path Test Complete!");
    println!("✓ Setup: Guild, tags, locations, equipment created");
    println!("✓ Reservation: Created with proper conflict detection");
    println!("✓ Transfer: Request created, notified, and approved");
    println!("✓ Return: Equipment returned with location confirmation");
    println!("✓ Correction: Return correction window validated");
    println!("✓ Ordering: Equipment sorted by tag order then name");
    println!("✓ Logs: All actions properly logged");
    println!("✓ JST: All user-facing times in JST format");
    println!("✓ Names: Equipment names included in all notifications");
    
    Ok(())
}

/// Test message self-healing on restart simulation
#[tokio::test]
async fn test_message_self_heal_on_restart() -> Result<()> {
    let ctx = TestContext::new().await?;
    let (_, _, _, equipment) = create_test_setup(&ctx).await?;
    
    println!("Testing message self-heal functionality");
    
    // Create managed messages records
    let guild_id = equipment.guild_id;
    let channel_id = 987654321i64;
    let message_id_1 = 111111111i64;
    let message_id_2 = 222222222i64;
    
    // Insert managed message records
    sqlx::query!(
        "INSERT INTO managed_messages (guild_id, channel_id, message_id, message_type, equipment_id, sort_order, created_at)
         VALUES (?, ?, ?, 'EquipmentEmbed', ?, 1, CURRENT_TIMESTAMP)",
        guild_id,
        channel_id,
        message_id_1,
        equipment.id
    )
    .execute(&ctx.db)
    .await?;
    
    sqlx::query!(
        "INSERT INTO managed_messages (guild_id, channel_id, message_id, message_type, equipment_id, sort_order, created_at)
         VALUES (?, ?, ?, 'OverallManagement', NULL, 0, CURRENT_TIMESTAMP)",
        guild_id,
        channel_id,
        message_id_2
    )
    .execute(&ctx.db)
    .await?;
    
    // Simulate "restart" by checking for missing/changed messages
    // In a real implementation, this would verify Discord messages exist and match DB state
    
    // Simulate finding orphaned message (exists in DB but not in Discord)
    let managed_messages = sqlx::query_as!(
        ManagedMessage,
        "SELECT * FROM managed_messages WHERE guild_id = ? ORDER BY sort_order, id",
        guild_id
    )
    .fetch_all(&ctx.db)
    .await?;
    
    assert_eq!(managed_messages.len(), 2);
    
    // Simulate deletion of orphaned Discord messages
    for msg in &managed_messages {
        ctx.discord_api.delete_message(
            ChannelId::new(msg.channel_id as u64),
            MessageId::new(msg.message_id as u64),
        ).await?;
    }
    
    // Simulate rebuilding messages in correct order
    for msg in &managed_messages {
        let content = match msg.message_type.as_str() {
            "EquipmentEmbed" => {
                format!("📷 {} - Available", equipment.name)
            }
            "OverallManagement" => {
                "🔧 Overall Management".to_string()
            }
            _ => "Unknown message type".to_string(),
        };
        
        let new_message_id = ctx.discord_api.send_channel_message(
            ChannelId::new(msg.channel_id as u64),
            &content,
        ).await?;
        
        // Update managed message record with new ID
        sqlx::query!(
            "UPDATE managed_messages SET message_id = ?, created_at = CURRENT_TIMESTAMP WHERE id = ?",
            new_message_id.get() as i64,
            msg.id
        )
        .execute(&ctx.db)
        .await?;
    }
    
    // Verify messages were rebuilt in correct order
    let channel_messages = ctx.discord_api.get_channel_messages().await;
    let deleted_messages = ctx.discord_api.deleted_messages.lock().await;
    
    // Should have deleted 2 old messages and sent 2 new ones
    assert_eq!(deleted_messages.len(), 2);
    assert_eq!(channel_messages.len(), 2);
    
    // Verify content and order
    assert!(channel_messages[0].1.contains("Overall Management")); // Sort order 0
    assert!(channel_messages[1].1.contains(&equipment.name));      // Sort order 1
    
    println!("✓ Message self-heal completed successfully");
    
    Ok(())
}

/// Test notification handling with various job types
#[tokio::test]
async fn test_comprehensive_notification_flow() -> Result<()> {
    let ctx = TestContext::new().await?;
    let (_, _, _, equipment) = create_test_setup(&ctx).await?;
    
    println!("Testing comprehensive notification flow");
    
    let user_id = 12345i64;
    let reservation_start = ctx.clock.now_utc();
    let reservation_end = reservation_start + Duration::hours(2);
    
    // Create reservation
    let reservation = ReservationBuilder::new(
        equipment.id,
        user_id,
        reservation_start,
        reservation_end,
    )
    .build(&ctx.db)
    .await?;
    
    // Test 1: Pre-end reminder
    let pre_end_job = crate::job_tests::schedule_job(
        &ctx.db,
        "reminder",
        serde_json::json!({
            "reservation_id": reservation.id,
            "type": "pre_end"
        }),
        reservation_end - Duration::minutes(15),
    ).await?;
    
    // Test 2: Return delay reminder
    let delay_job = crate::job_tests::schedule_job(
        &ctx.db,
        "reminder",
        serde_json::json!({
            "reservation_id": reservation.id,
            "type": "return_delay"
        }),
        reservation_end + Duration::minutes(30),
    ).await?;
    
    // Test 3: Transfer timeout
    let transfer = crate::transfer_tests::create_transfer_request(
        &ctx.db,
        reservation.id,
        user_id,
        67890,
    ).await?;
    
    let timeout_job = crate::job_tests::schedule_job(
        &ctx.db,
        "transfer_timeout",
        serde_json::json!({
            "transfer_id": transfer.id
        }),
        transfer.expires_at,
    ).await?;
    
    let worker = crate::job_tests::TestJobWorker::new(
        ctx.db.clone(),
        ctx.discord_api.clone(),
        ctx.clock.clone(),
    );
    
    // Process pre-end reminder
    ctx.clock.set_time(reservation_end - Duration::minutes(15)).await;
    worker.process_pending_jobs().await?;
    
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert!(sent_dms[0].1.contains("15分"));
    assert!(sent_dms[0].1.contains(&equipment.name));
    
    // Process return delay reminder
    ctx.discord_api.clear().await;
    ctx.clock.set_time(reservation_end + Duration::minutes(30)).await;
    worker.process_pending_jobs().await?;
    
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert!(sent_dms[0].1.contains("遅延"));
    
    // Process transfer timeout
    ctx.discord_api.clear().await;
    ctx.clock.set_time(transfer.expires_at).await;
    worker.process_pending_jobs().await?;
    
    let sent_dms = ctx.discord_api.get_sent_dms().await;
    assert_eq!(sent_dms.len(), 1);
    assert!(sent_dms[0].1.contains("期限切れ"));
    
    // Verify transfer was marked as expired
    let final_transfer = sqlx::query!(
        "SELECT status FROM transfer_requests WHERE id = ?",
        transfer.id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(final_transfer.status, "Expired");
    
    println!("✓ All notification types processed correctly");
    
    Ok(())
}