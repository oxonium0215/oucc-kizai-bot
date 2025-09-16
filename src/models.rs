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
    pub expires_at: DateTime<Utc>,
    pub status: String, // Pending, Accepted, Denied, Expired
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
}

impl From<String> for TransferStatus {
    fn from(s: String) -> Self {
        match s.as_str() {
            "Pending" => Self::Pending,
            "Accepted" => Self::Accepted,
            "Denied" => Self::Denied,
            "Expired" => Self::Expired,
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
}

impl From<String> for MessageType {
    fn from(s: String) -> Self {
        match s.as_str() {
            "EquipmentEmbed" => Self::EquipmentEmbed,
            "OverallManagement" => Self::OverallManagement,
            "Guide" => Self::Guide,
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
        }
    }
}
