use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Guild {
    pub id: i64,
    pub reservation_channel_id: Option<i64>,
    pub admin_roles: Option<String>, // JSON array of role IDs
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    // Notification preferences
    pub dm_fallback_channel_enabled: Option<bool>,
    pub overdue_repeat_hours: Option<i64>,
    pub overdue_max_count: Option<i64>,
    pub pre_start_minutes: Option<i64>,
    pub pre_end_minutes: Option<i64>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Tag {
    pub id: i64,
    pub guild_id: i64,
    pub name: String,
    pub sort_order: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Equipment {
    pub id: i64,
    pub guild_id: i64,
    pub tag_id: Option<i64>,

    pub name: String,
    pub status: String, // Available, Loaned, Unavailable
    pub current_location: Option<String>,
    pub unavailable_reason: Option<String>,
    pub default_return_location: Option<String>,
    pub message_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Location {
    pub id: i64,
    pub guild_id: i64,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Reservation {
    pub id: i64,
    pub equipment_id: i64,
    pub user_id: i64,
    pub start_time: DateTime<Utc>,
    pub end_time: DateTime<Utc>,
    pub location: Option<String>,
    pub status: String, // Confirmed, Cancelled
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub returned_at: Option<DateTime<Utc>>,
    pub return_location: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct EquipmentLog {
    pub id: i64,
    pub equipment_id: i64,
    pub user_id: i64,
    pub action: String,
    pub location: Option<String>,
    pub previous_status: Option<String>,
    pub new_status: Option<String>,
    pub notes: Option<String>,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct TransferRequest {
    pub id: i64,
    pub reservation_id: i64,
    pub from_user_id: i64,
    pub to_user_id: i64,
    pub requested_by_user_id: i64,
    pub execute_at_utc: Option<DateTime<Utc>>, // NULL for immediate transfers
    pub note: Option<String>,
    pub expires_at: DateTime<Utc>,
    pub status: String, // Pending, Accepted, Denied, Expired, Canceled
    pub canceled_at_utc: Option<DateTime<Utc>>,
    pub canceled_by_user_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct Job {
    pub id: i64,
    pub job_type: String,
    pub payload: String, // JSON payload
    pub scheduled_for: DateTime<Utc>,
    pub status: String, // Pending, Running, Completed, Failed
    pub attempts: i64,
    pub max_attempts: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct ManagedMessage {
    pub id: i64,
    pub guild_id: i64,
    pub channel_id: i64,
    pub message_id: i64,
    pub message_type: String, // EquipmentEmbed, OverallManagement, Guide
    pub equipment_id: Option<i64>,
    pub sort_order: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct SentReminder {
    pub id: i64,
    pub reservation_id: i64,
    pub kind: String,
    pub sent_at_utc: DateTime<Utc>,
    pub delivery_method: String, // DM, CHANNEL, FAILED
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MaintenanceWindow {
    pub id: i64,
    pub equipment_id: i64,
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
    pub reason: Option<String>,
    pub created_by_user_id: i64,
    pub created_at_utc: DateTime<Utc>,
    pub canceled_at_utc: Option<DateTime<Utc>>,
    pub canceled_by_user_id: Option<i64>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize)]
pub struct MaintenanceSettings {
    pub guild_id: i64,
    pub admin_reminder_minutes: Option<i64>,
}



// Enums for better type safety
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EquipmentStatus {
    Available,
    Loaned,
    Unavailable,
}

impl From<String> for EquipmentStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "Available" => Self::Available,
            "Loaned" => Self::Loaned,
            "Unavailable" => Self::Unavailable,
            _ => Self::Available,
        }
    }
}

impl From<EquipmentStatus> for String {
    fn from(status: EquipmentStatus) -> Self {
        match status {
            EquipmentStatus::Available => "Available".to_string(),
            EquipmentStatus::Loaned => "Loaned".to_string(),
            EquipmentStatus::Unavailable => "Unavailable".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReservationStatus {
    Confirmed,
    Cancelled,
}

impl From<String> for ReservationStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "Confirmed" => Self::Confirmed,
            "Cancelled" => Self::Cancelled,
            _ => Self::Confirmed,
        }
    }
}

impl From<ReservationStatus> for String {
    fn from(status: ReservationStatus) -> Self {
        match status {
            ReservationStatus::Confirmed => "Confirmed".to_string(),
            ReservationStatus::Cancelled => "Cancelled".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    Accepted,
    Denied,
    Expired,
    Canceled,
}

impl From<String> for TransferStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "Pending" => Self::Pending,
            "Accepted" => Self::Accepted,
            "Denied" => Self::Denied,
            "Expired" => Self::Expired,
            "Canceled" => Self::Canceled,
            _ => Self::Pending,
        }
    }
}

impl From<TransferStatus> for String {
    fn from(status: TransferStatus) -> Self {
        match status {
            TransferStatus::Pending => "Pending".to_string(),
            TransferStatus::Accepted => "Accepted".to_string(),
            TransferStatus::Denied => "Denied".to_string(),
            TransferStatus::Expired => "Expired".to_string(),
            TransferStatus::Canceled => "Canceled".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl From<String> for JobStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "Pending" => Self::Pending,
            "Running" => Self::Running,
            "Completed" => Self::Completed,
            "Failed" => Self::Failed,
            _ => Self::Pending,
        }
    }
}

impl From<JobStatus> for String {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => "Pending".to_string(),
            JobStatus::Running => "Running".to_string(),
            JobStatus::Completed => "Completed".to_string(),
            JobStatus::Failed => "Failed".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MessageType {
    EquipmentEmbed,
    OverallManagement,
    Guide,
    Header,
}

impl From<String> for MessageType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "EquipmentEmbed" => Self::EquipmentEmbed,
            "OverallManagement" => Self::OverallManagement,
            "Guide" => Self::Guide,
            "Header" => Self::Header,
            _ => Self::Guide,
        }
    }
}

impl From<MessageType> for String {
    fn from(msg_type: MessageType) -> Self {
        match msg_type {
            MessageType::EquipmentEmbed => "EquipmentEmbed".to_string(),
            MessageType::OverallManagement => "OverallManagement".to_string(),
            MessageType::Guide => "Guide".to_string(),
            MessageType::Header => "Header".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReminderKind {
    PreStart,
    Start,
    PreEnd,
    Overdue(u32), // Number for the overdue reminder sequence (1, 2, 3, etc.)
}

impl ReminderKind {
    pub fn to_db_string(&self) -> String {
        match self {
            Self::PreStart => "PRE_START".to_string(),
            Self::Start => "START".to_string(),
            Self::PreEnd => "PRE_END".to_string(),
            Self::Overdue(n) => format!("OVERDUE_{}", n),
        }
    }

    pub fn from_db_string(s: &str) -> Option<Self> {
        match s {
            "PRE_START" => Some(Self::PreStart),
            "START" => Some(Self::Start),
            "PRE_END" => Some(Self::PreEnd),
            s if s.starts_with("OVERDUE_") => {
                let num_part = s.strip_prefix("OVERDUE_")?;
                let num: u32 = num_part.parse().ok()?;
                Some(Self::Overdue(num))
            }
            _ => None,
        }
    }
}

impl From<String> for ReminderKind {
    fn from(s: String) -> Self {
        Self::from_db_string(&s).unwrap_or(Self::PreStart)
    }
}

impl From<ReminderKind> for String {
    fn from(kind: ReminderKind) -> Self {
        kind.to_db_string()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeliveryMethod {
    Dm,
    Channel,
    Failed,
}

impl From<String> for DeliveryMethod {
    fn from(s: String) -> Self {
        match s.as_str() {
            "DM" => Self::Dm,
            "CHANNEL" => Self::Channel,
            "FAILED" => Self::Failed,
            _ => Self::Failed,
        }
    }
}

impl From<DeliveryMethod> for String {
    fn from(method: DeliveryMethod) -> Self {
        match method {
            DeliveryMethod::Dm => "DM".to_string(),
            DeliveryMethod::Channel => "CHANNEL".to_string(),
            DeliveryMethod::Failed => "FAILED".to_string(),
        }
    }
}
