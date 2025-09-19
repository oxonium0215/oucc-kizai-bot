use chrono::{Duration, Utc};
use oucc_kizai_bot::constants::Constants;

/// Test equipment status constants
#[test]
fn test_equipment_status_constants() {
    assert_eq!(Constants::EQUIPMENT_AVAILABLE, "Available");
    assert_eq!(Constants::EQUIPMENT_LOANED, "Loaned");
    assert_eq!(Constants::EQUIPMENT_UNAVAILABLE, "Unavailable");
}

/// Test reservation status constants
#[test]
fn test_reservation_status_constants() {
    assert_eq!(Constants::STATUS_CONFIRMED, "Confirmed");
    assert_eq!(Constants::STATUS_PENDING, "Pending");
    assert_eq!(Constants::STATUS_CANCELED, "Canceled");
}

/// Test transfer status constants
#[test]
fn test_transfer_status_constants() {
    assert_eq!(Constants::TRANSFER_PENDING, "Pending");
    assert_eq!(Constants::TRANSFER_ACCEPTED, "Accepted");
    assert_eq!(Constants::TRANSFER_DENIED, "Denied");
    assert_eq!(Constants::TRANSFER_EXPIRED, "Expired");
    assert_eq!(Constants::TRANSFER_CANCELED, "Canceled");
}

/// Test log action constants
#[test]
fn test_log_action_constants() {
    // Basic reservation actions
    assert_eq!(Constants::LOG_ACTION_RESERVE, "reserve");
    assert_eq!(Constants::LOG_ACTION_RETURN, "return");
    assert_eq!(Constants::LOG_ACTION_CANCEL, "cancel");
    assert_eq!(Constants::LOG_ACTION_TRANSFER, "transfer");
    assert_eq!(Constants::LOG_ACTION_EDIT, "edit");
    assert_eq!(Constants::LOG_ACTION_FORCE_STATE, "force_state");

    // Equipment management actions
    assert_eq!(Constants::LOG_ACTION_CREATE_EQUIPMENT, "eq_create");
    assert_eq!(Constants::LOG_ACTION_DELETE_EQUIPMENT, "eq_delete");
    assert_eq!(Constants::LOG_ACTION_RENAME_EQUIPMENT, "eq_rename");
    assert_eq!(Constants::LOG_ACTION_ASSIGN_TAG, "eq_assign_tag");
    assert_eq!(Constants::LOG_ACTION_SET_LOCATION, "eq_set_location");
    assert_eq!(Constants::LOG_ACTION_SET_UNAVAILABLE, "eq_set_unavailable");

    // Overall management actions
    assert_eq!(Constants::LOG_ACTION_MGMT_ADD_EQUIPMENT, "mgmt_add_equipment");
    assert_eq!(Constants::LOG_ACTION_MGMT_ADD_TAG, "mgmt_add_tag");
    assert_eq!(Constants::LOG_ACTION_MGMT_DELETE_TAG, "mgmt_delete_tag");
    assert_eq!(Constants::LOG_ACTION_MGMT_REORDER_TAG, "mgmt_reorder_tag");
    assert_eq!(Constants::LOG_ACTION_MGMT_ADD_LOCATION, "mgmt_add_location");
    assert_eq!(Constants::LOG_ACTION_MGMT_DELETE_LOCATION, "mgmt_delete_location");
}

/// Test page size constants
#[test]
fn test_page_size_constants() {
    assert_eq!(Constants::DEFAULT_MANAGEMENT_PAGE_SIZE, 10);
    assert_eq!(Constants::DEFAULT_LOG_PAGE_SIZE, 15);
}

/// Test time constants
#[test]
fn test_time_constants() {
    assert_eq!(Constants::TRANSFER_TIMEOUT_HOURS, 3);
    assert_eq!(Constants::RETURN_CORRECTION_WINDOW_HOURS, 1);
    assert_eq!(Constants::NEXT_RESERVATION_BUFFER_MINUTES, 15);
    assert_eq!(Constants::PRE_END_NOTIFICATION_MINUTES, 15);
    
    // Test session cleanup constants
    assert_eq!(Constants::SESSION_CLEANUP_INTERVAL_MINUTES, 30);
    assert_eq!(Constants::SESSION_EXPIRY_HOURS, 2);
}

/// Test message format constants
#[test]
fn test_message_constants() {
    assert!(Constants::MSG_ADMIN_REQUIRED.starts_with("❌"));
    assert!(Constants::MSG_ADMIN_LOG_REQUIRED.starts_with("❌"));
    assert!(Constants::MSG_OPERATION_SUCCESS.starts_with("✅"));
    assert!(Constants::MSG_RESERVATION_CREATED.starts_with("✅"));
}

/// Test emoji constants
#[test]
fn test_emoji_constants() {
    assert_eq!(Constants::SUCCESS_EMOJI, "✅");
    assert_eq!(Constants::ERROR_EMOJI, "❌");
    assert_eq!(Constants::CALENDAR_EMOJI, "📅");
    assert_eq!(Constants::LOCATION_EMOJI, "📍");
    assert_eq!(Constants::LOG_EMOJI, "📋");
    
    // Status emojis
    assert_eq!(Constants::AVAILABLE_EMOJI, "✅");
    assert_eq!(Constants::LOANED_EMOJI, "🔒");
    assert_eq!(Constants::UNAVAILABLE_EMOJI, "❌");
}

/// Test that critical length limits are reasonable
#[test]
fn test_length_limits() {
    assert!(Constants::MAX_EQUIPMENT_NAME_LENGTH >= 50, "Equipment names should allow reasonable length");
    assert!(Constants::MAX_LOCATION_NAME_LENGTH >= 20, "Location names should allow reasonable length");
    assert!(Constants::MAX_TAG_NAME_LENGTH >= 10, "Tag names should allow reasonable length");
    assert!(Constants::MAX_UNAVAILABLE_REASON_LENGTH >= 100, "Unavailable reasons should allow sufficient detail");
    assert!(Constants::MAX_TRANSFER_NOTE_LENGTH >= 200, "Transfer notes should allow sufficient detail");
}

/// Test time format constants
#[test]
fn test_time_format_constants() {
    assert_eq!(Constants::JST_DATETIME_FORMAT, "%Y/%m/%d %H:%M");
    assert_eq!(Constants::JST_DATE_FORMAT, "%Y/%m/%d");
    assert_eq!(Constants::JST_TIME_FORMAT, "%H:%M");
}

/// Test that Constants struct provides time-related constants for return correction
#[test]
fn test_return_correction_constants() {
    // Verify the time constants are reasonable
    assert_eq!(Constants::RETURN_CORRECTION_WINDOW_HOURS, 1);
    assert_eq!(Constants::NEXT_RESERVATION_BUFFER_MINUTES, 15);
    
    // Test basic duration calculations would work with our constants
    let correction_window = Duration::hours(Constants::RETURN_CORRECTION_WINDOW_HOURS);
    let buffer = Duration::minutes(Constants::NEXT_RESERVATION_BUFFER_MINUTES);
    
    assert_eq!(correction_window.num_hours(), 1);
    assert_eq!(buffer.num_minutes(), 15);
}

/// Test log action coverage by checking all constants exist
#[test] 
fn test_log_action_coverage() {
    let actions = vec![
        Constants::LOG_ACTION_RESERVE,
        Constants::LOG_ACTION_RETURN,
        Constants::LOG_ACTION_CANCEL,
        Constants::LOG_ACTION_TRANSFER,
        Constants::LOG_ACTION_EDIT,
        Constants::LOG_ACTION_FORCE_STATE,
        Constants::LOG_ACTION_CREATE_EQUIPMENT,
        Constants::LOG_ACTION_DELETE_EQUIPMENT,
        Constants::LOG_ACTION_RENAME_EQUIPMENT,
        Constants::LOG_ACTION_ASSIGN_TAG,
        Constants::LOG_ACTION_SET_LOCATION,
        Constants::LOG_ACTION_SET_UNAVAILABLE,
        Constants::LOG_ACTION_MGMT_ADD_EQUIPMENT,
        Constants::LOG_ACTION_MGMT_ADD_TAG,
        Constants::LOG_ACTION_MGMT_DELETE_TAG,
        Constants::LOG_ACTION_MGMT_REORDER_TAG,
        Constants::LOG_ACTION_MGMT_ADD_LOCATION,
        Constants::LOG_ACTION_MGMT_DELETE_LOCATION,
    ];

    // Ensure all actions are unique
    let unique_actions: std::collections::HashSet<_> = actions.iter().collect();
    assert_eq!(unique_actions.len(), actions.len(), "All log actions should be unique");

    // Ensure no action is empty
    for action in &actions {
        assert!(!action.is_empty(), "No log action should be empty");
    }
}