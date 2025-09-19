/// Constants used throughout the application for consistency
pub struct Constants;

impl Constants {
    // Emoji markers for consistent UX
    pub const SUCCESS_EMOJI: &'static str = "✅";
    pub const ERROR_EMOJI: &'static str = "❌";
    pub const CALENDAR_EMOJI: &'static str = "📅";
    pub const LOCATION_EMOJI: &'static str = "📍";
    pub const LOG_EMOJI: &'static str = "📋";
    pub const SETTINGS_EMOJI: &'static str = "⚙️";
    pub const DELETE_EMOJI: &'static str = "🗑️";
    pub const TRANSFER_EMOJI: &'static str = "🔄";
    pub const LOCK_EMOJI: &'static str = "🔒";
    pub const QUESTION_EMOJI: &'static str = "❓";
    pub const WARNING_EMOJI: &'static str = "⚠️";
    pub const CLOCK_EMOJI: &'static str = "🕐";
    pub const RETURN_EMOJI: &'static str = "↩️";
    pub const CONSTRUCTION_EMOJI: &'static str = "🚧";

    // Status indicators
    pub const AVAILABLE_EMOJI: &'static str = "✅";
    pub const LOANED_EMOJI: &'static str = "🔒";
    pub const UNAVAILABLE_EMOJI: &'static str = "❌";

    // Page sizes and limits
    pub const DEFAULT_MANAGEMENT_PAGE_SIZE: usize = 10;
    pub const DEFAULT_LOG_PAGE_SIZE: usize = 15;
    pub const MAX_EQUIPMENT_NAME_LENGTH: usize = 100;
    pub const MAX_LOCATION_NAME_LENGTH: usize = 50;
    pub const MAX_TAG_NAME_LENGTH: usize = 30;
    pub const MAX_UNAVAILABLE_REASON_LENGTH: usize = 200;
    pub const MAX_TRANSFER_NOTE_LENGTH: usize = 500;

    // Time constants (in hours)
    pub const TRANSFER_TIMEOUT_HOURS: i64 = 3;
    pub const RETURN_CORRECTION_WINDOW_HOURS: i64 = 1;
    pub const NEXT_RESERVATION_BUFFER_MINUTES: i64 = 15;
    pub const PRE_END_NOTIFICATION_MINUTES: i64 = 15;

    // Reservation status
    pub const STATUS_CONFIRMED: &'static str = "Confirmed";
    pub const STATUS_PENDING: &'static str = "Pending";
    pub const STATUS_CANCELED: &'static str = "Canceled";

    // Equipment status
    pub const EQUIPMENT_AVAILABLE: &'static str = "Available";
    pub const EQUIPMENT_LOANED: &'static str = "Loaned";
    pub const EQUIPMENT_UNAVAILABLE: &'static str = "Unavailable";

    // Transfer request status
    pub const TRANSFER_PENDING: &'static str = "Pending";
    pub const TRANSFER_ACCEPTED: &'static str = "Accepted";
    pub const TRANSFER_DENIED: &'static str = "Denied";
    pub const TRANSFER_EXPIRED: &'static str = "Expired";
    pub const TRANSFER_CANCELED: &'static str = "Canceled";

    // Log actions
    pub const LOG_ACTION_RESERVE: &'static str = "reserve";
    pub const LOG_ACTION_RETURN: &'static str = "return";
    pub const LOG_ACTION_CANCEL: &'static str = "cancel";
    pub const LOG_ACTION_TRANSFER: &'static str = "transfer";
    pub const LOG_ACTION_EDIT: &'static str = "edit";
    pub const LOG_ACTION_FORCE_STATE: &'static str = "force_state";
    pub const LOG_ACTION_CREATE_EQUIPMENT: &'static str = "eq_create";
    pub const LOG_ACTION_DELETE_EQUIPMENT: &'static str = "eq_delete";
    pub const LOG_ACTION_RENAME_EQUIPMENT: &'static str = "eq_rename";
    pub const LOG_ACTION_ASSIGN_TAG: &'static str = "eq_assign_tag";
    pub const LOG_ACTION_SET_LOCATION: &'static str = "eq_set_location";
    pub const LOG_ACTION_SET_UNAVAILABLE: &'static str = "eq_set_unavailable";
    pub const LOG_ACTION_MGMT_ADD_EQUIPMENT: &'static str = "mgmt_add_equipment";
    pub const LOG_ACTION_MGMT_ADD_TAG: &'static str = "mgmt_add_tag";
    pub const LOG_ACTION_MGMT_DELETE_TAG: &'static str = "mgmt_delete_tag";
    pub const LOG_ACTION_MGMT_REORDER_TAG: &'static str = "mgmt_reorder_tag";
    pub const LOG_ACTION_MGMT_ADD_LOCATION: &'static str = "mgmt_add_location";
    pub const LOG_ACTION_MGMT_DELETE_LOCATION: &'static str = "mgmt_delete_location";

    // Error messages
    pub const MSG_ADMIN_REQUIRED: &'static str = "❌ You need administrator permissions to use this feature.";
    pub const MSG_ADMIN_LOG_REQUIRED: &'static str = "❌ You need administrator permissions to view operation logs.";
    pub const MSG_INVALID_TIME_FORMAT: &'static str = "❌ Invalid time format. Please use YYYY/MM/DD HH:MM format.";
    pub const MSG_EQUIPMENT_NOT_FOUND: &'static str = "❌ Equipment not found.";
    pub const MSG_RESERVATION_NOT_FOUND: &'static str = "❌ Reservation not found.";
    pub const MSG_OPERATION_NOT_POSSIBLE: &'static str = "❌ Operation is not possible because the next reservation is imminent or because 1 hour has passed since return.";

    // Success messages
    pub const MSG_OPERATION_SUCCESS: &'static str = "✅ Operation completed successfully.";
    pub const MSG_RESERVATION_CREATED: &'static str = "✅ Reservation created successfully.";
    pub const MSG_RESERVATION_CANCELED: &'static str = "✅ Reservation canceled successfully.";
    pub const MSG_EQUIPMENT_RETURNED: &'static str = "✅ Equipment returned successfully.";

    // Time display format
    pub const JST_DATETIME_FORMAT: &'static str = "%Y/%m/%d %H:%M";
    pub const JST_DATE_FORMAT: &'static str = "%Y/%m/%d";
    pub const JST_TIME_FORMAT: &'static str = "%H:%M";
}