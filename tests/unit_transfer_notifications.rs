use oucc_kizai_bot::transfer_notifications::TransferNotificationType;

#[test]
fn test_notification_message_content() {
    // Test that notification messages contain appropriate content
    
    let approved = TransferNotificationType::Approved {
        equipment_name: "Camera A".to_string(),
    };
    assert!(approved.dm_message().contains("移譲承認通知"));
    assert!(approved.dm_message().contains("Camera A"));

    let denied = TransferNotificationType::Denied {
        equipment_name: "Camera A".to_string(),
        reason: "Unavailable".to_string(),
    };
    assert!(denied.dm_message().contains("移譲拒否通知"));
    assert!(denied.dm_message().contains("Camera A"));
    assert!(denied.dm_message().contains("Unavailable"));

    let expired = TransferNotificationType::Expired {
        equipment_name: "Camera A".to_string(),
    };
    assert!(expired.dm_message().contains("移譲期限切れ通知"));
    assert!(expired.dm_message().contains("Camera A"));
    assert!(expired.dm_message().contains("3時間"));

    let request = TransferNotificationType::RequestSent {
        equipment_name: "Camera A".to_string(),
        requester_id: 123,
        reservation_id: 456,
    };
    assert!(request.dm_message().contains("予約移譲依頼"));
    assert!(request.dm_message().contains("Camera A"));

    // Test fallback messages don't contain sensitive info
    assert!(!request.fallback_message(456).contains("期間"));
    assert!(!request.fallback_message(456).contains("場所"));
    assert!(request.fallback_message(456).contains("Camera A"));
    assert!(request.fallback_message(456).contains("予約ID: #456"));
}

#[test]
fn test_equipment_name_extraction() {
    let notifications = vec![
        TransferNotificationType::Approved { equipment_name: "Camera A".to_string() },
        TransferNotificationType::Denied { 
            equipment_name: "Camera B".to_string(), 
            reason: "Test".to_string() 
        },
        TransferNotificationType::Expired { equipment_name: "Camera C".to_string() },
        TransferNotificationType::RequestSent { 
            equipment_name: "Camera D".to_string(), 
            requester_id: 123, 
            reservation_id: 456 
        },
        TransferNotificationType::Cancelled { 
            equipment_name: "Camera E".to_string(), 
            canceller_id: 789 
        },
    ];

    let expected_names = vec!["Camera A", "Camera B", "Camera C", "Camera D", "Camera E"];
    
    for (notification, expected) in notifications.iter().zip(expected_names.iter()) {
        assert_eq!(notification.equipment_name(), *expected);
    }
}

#[test]
fn test_fallback_message_security() {
    // Ensure fallback messages never contain sensitive information
    let notifications = vec![
        TransferNotificationType::Approved { equipment_name: "Secret Equipment".to_string() },
        TransferNotificationType::Denied { 
            equipment_name: "Secret Equipment".to_string(), 
            reason: "Confidential reason".to_string() 
        },
        TransferNotificationType::Expired { equipment_name: "Secret Equipment".to_string() },
        TransferNotificationType::RequestSent { 
            equipment_name: "Secret Equipment".to_string(), 
            requester_id: 123, 
            reservation_id: 456 
        },
        TransferNotificationType::Cancelled { 
            equipment_name: "Secret Equipment".to_string(), 
            canceller_id: 789 
        },
    ];

    for notification in notifications {
        let fallback_msg = notification.fallback_message(42);
        
        // Should contain equipment name and reservation ID
        assert!(fallback_msg.contains("Secret Equipment"));
        assert!(fallback_msg.contains("予約ID: #42"));
        
        // Should NOT contain sensitive details
        assert!(!fallback_msg.contains("期間"));      // Period/duration
        assert!(!fallback_msg.contains("時刻"));      // Time  
        assert!(!fallback_msg.contains("場所"));      // Location
        assert!(!fallback_msg.contains("メモ"));      // Note
        assert!(!fallback_msg.contains("JST"));       // Time zone
        assert!(!fallback_msg.contains("Confidential reason")); // Private reason
    }
}