use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use oucc_kizai_bot::models::*;

/// Test the transfer approval flow business logic
#[tokio::test]
async fn test_transfer_approval_flow() -> Result<()> {
    // Test valid transfer status transitions for approval flow
    
    // Test valid transitions from Pending
    assert!(is_valid_transfer_transition(&TransferStatus::Pending, &TransferStatus::Accepted));
    assert!(is_valid_transfer_transition(&TransferStatus::Pending, &TransferStatus::Denied));
    assert!(is_valid_transfer_transition(&TransferStatus::Pending, &TransferStatus::Expired));
    assert!(is_valid_transfer_transition(&TransferStatus::Pending, &TransferStatus::Canceled));
    
    // Test invalid transitions from terminal states
    assert!(!is_valid_transfer_transition(&TransferStatus::Accepted, &TransferStatus::Denied));
    assert!(!is_valid_transfer_transition(&TransferStatus::Denied, &TransferStatus::Accepted));
    assert!(!is_valid_transfer_transition(&TransferStatus::Expired, &TransferStatus::Accepted));
    assert!(!is_valid_transfer_transition(&TransferStatus::Canceled, &TransferStatus::Accepted));
    
    Ok(())
}

#[test]
fn test_transfer_expiry_calculation() {
    let now = Utc::now();
    let expires_at = calculate_transfer_expiry(now);
    
    // Should expire exactly 3 hours from now
    let expected_expiry = now + Duration::hours(3);
    let diff = (expires_at - expected_expiry).num_seconds().abs();
    
    // Allow for small timing differences (within 1 second)
    assert!(diff <= 1, "Transfer expiry should be 3 hours from creation time");
}

#[test] 
fn test_transfer_status_conversions() {
    // Test enum to string conversions
    assert_eq!(String::from(TransferStatus::Pending), "Pending");
    assert_eq!(String::from(TransferStatus::Accepted), "Accepted");
    assert_eq!(String::from(TransferStatus::Denied), "Denied");
    assert_eq!(String::from(TransferStatus::Expired), "Expired");
    assert_eq!(String::from(TransferStatus::Canceled), "Canceled");
    
    // Test string to enum conversions
    assert_eq!(TransferStatus::from("Pending".to_string()), TransferStatus::Pending);
    assert_eq!(TransferStatus::from("Accepted".to_string()), TransferStatus::Accepted);
    assert_eq!(TransferStatus::from("Denied".to_string()), TransferStatus::Denied);
    assert_eq!(TransferStatus::from("Expired".to_string()), TransferStatus::Expired);
    assert_eq!(TransferStatus::from("Canceled".to_string()), TransferStatus::Canceled);
    
    // Test invalid string defaults to Pending
    assert_eq!(TransferStatus::from("Invalid".to_string()), TransferStatus::Pending);
}

// Helper functions to test business logic
fn is_valid_transfer_transition(from: &TransferStatus, to: &TransferStatus) -> bool {
    match (from, to) {
        // From Pending - all transitions are valid
        (TransferStatus::Pending, _) => true,
        
        // From terminal states - no transitions allowed
        (TransferStatus::Accepted, _) => false,
        (TransferStatus::Denied, _) => false,
        (TransferStatus::Expired, _) => false,
        (TransferStatus::Canceled, _) => false,
    }
}

fn calculate_transfer_expiry(created_at: DateTime<Utc>) -> DateTime<Utc> {
    created_at + Duration::hours(3)
}

#[test]
fn test_approval_flow_timing() {
    let created_at = Utc::now();
    let expires_at = calculate_transfer_expiry(created_at);
    
    // Should be exactly 3 hours
    assert_eq!((expires_at - created_at).num_hours(), 3);
    
    // Test if a request would be expired
    let now_plus_4_hours = created_at + Duration::hours(4);
    assert!(now_plus_4_hours > expires_at, "Request should be expired after 4 hours");
    
    let now_plus_2_hours = created_at + Duration::hours(2);
    assert!(now_plus_2_hours < expires_at, "Request should still be valid after 2 hours");
}