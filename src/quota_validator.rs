use anyhow::Result;
use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

/// Simple quota validation result for integration
#[derive(Debug, Clone)]
pub enum QuotaValidationResult {
    Success,
    Exceeded { message: String },
}

impl QuotaValidationResult {
    pub fn is_success(&self) -> bool {
        matches!(self, QuotaValidationResult::Success)
    }

    pub fn error_message(&self) -> Option<String> {
        match self {
            QuotaValidationResult::Success => None,
            QuotaValidationResult::Exceeded { message } => Some(message.clone()),
        }
    }
}

/// Simple quota validator for integration into existing flows
pub struct QuotaValidator {
    db: SqlitePool,
}

impl QuotaValidator {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Check if user can make a reservation within quota limits
    /// This is a simplified version that can be called from handlers
    pub async fn validate_reservation_quota(
        &self,
        guild_id: i64,
        user_id: i64,
        user_roles: &[i64],
        proposed_start: DateTime<Utc>,
        proposed_end: DateTime<Utc>,
        exclude_reservation_id: Option<i64>,
    ) -> Result<QuotaValidationResult> {
        // Check if quotas are enabled for this guild
        let quota_enabled = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM quota_settings WHERE guild_id = ?",
            guild_id
        )
        .fetch_one(&self.db)
        .await?;

        if quota_enabled == 0 {
            return Ok(QuotaValidationResult::Success);
        }

        // Get base limits
        let limits = sqlx::query!(
            "SELECT max_active_count, max_overlap_count, max_hours_7d, max_hours_30d 
             FROM quota_settings WHERE guild_id = ?",
            guild_id
        )
        .fetch_optional(&self.db)
        .await?;

        let Some(limits) = limits else {
            return Ok(QuotaValidationResult::Success);
        };

        // Check active reservation count if limit exists
        if let Some(max_active) = limits.max_active_count {
            let active_count = self.get_active_count(user_id, exclude_reservation_id).await?;
            if active_count >= max_active {
                return Ok(QuotaValidationResult::Exceeded {
                    message: format!(
                        "❌ **Quota exceeded: Active reservations**\n\nYou have {} active reservations, but the limit is {}.\n\n💡 **Tip:** Return some equipment or wait for reservations to end before making new ones.",
                        active_count, max_active
                    ),
                });
            }
        }

        // Check overlap count if limit exists
        if let Some(max_overlap) = limits.max_overlap_count {
            let overlap_count = self.get_overlap_count(
                user_id,
                proposed_start,
                proposed_end,
                exclude_reservation_id,
            ).await?;
            
            if overlap_count >= max_overlap {
                return Ok(QuotaValidationResult::Exceeded {
                    message: format!(
                        "❌ **Quota exceeded: Simultaneous reservations**\n\nThis reservation would overlap with {} others, but the limit is {}.\n\n💡 **Tip:** Choose a different time slot or adjust your reservation times.",
                        overlap_count, max_overlap
                    ),
                });
            }
        }

        Ok(QuotaValidationResult::Success)
    }

    async fn get_active_count(&self, user_id: i64, exclude_id: Option<i64>) -> Result<i64> {
        let count = if let Some(exclude) = exclude_id {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed' AND returned_at IS NULL 
                 AND end_time > datetime('now') AND id != ?",
                user_id,
                exclude
            )
            .fetch_one(&self.db)
            .await?
        } else {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed' AND returned_at IS NULL 
                 AND end_time > datetime('now')",
                user_id
            )
            .fetch_one(&self.db)
            .await?
        };

        Ok(count as i64)
    }

    async fn get_overlap_count(
        &self,
        user_id: i64,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        exclude_id: Option<i64>,
    ) -> Result<i64> {
        let start_naive = start.naive_utc();
        let end_naive = end.naive_utc();

        let count = if let Some(exclude) = exclude_id {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed' AND returned_at IS NULL
                 AND start_time < ? AND end_time > ? AND id != ?",
                user_id,
                end_naive,
                start_naive,
                exclude
            )
            .fetch_one(&self.db)
            .await?
        } else {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed' AND returned_at IS NULL
                 AND start_time < ? AND end_time > ?",
                user_id,
                end_naive,
                start_naive
            )
            .fetch_one(&self.db)
            .await?
        };

        Ok(count as i64)
    }

    /// Record an admin override for quota validation
    pub async fn record_override(
        &self,
        guild_id: i64,
        reservation_id: Option<i64>,
        user_id: i64,
        admin_id: i64,
        reason: Option<String>,
    ) -> Result<i64> {
        let result = sqlx::query!(
            "INSERT INTO quota_override_audits 
             (guild_id, reservation_id, user_id, acted_by_user_id, reason)
             VALUES (?, ?, ?, ?, ?)",
            guild_id,
            reservation_id,
            user_id,
            admin_id,
            reason
        )
        .execute(&self.db)
        .await?;

        Ok(result.last_insert_rowid())
    }
}