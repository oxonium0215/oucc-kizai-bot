use anyhow::Result;
use chrono::{DateTime, Utc, Duration};
use sqlx::SqlitePool;
use crate::models::{QuotaSettings, QuotaRoleOverride, QuotaOverrideAudit, QuotaClassOverride, EffectiveQuotaLimits};

/// Helper functions for quota operations and validation
pub struct QuotaHelper {
    db: SqlitePool,
}

impl QuotaHelper {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Get or create quota settings for a guild
    pub async fn get_quota_settings(&self, guild_id: i64) -> Result<Option<QuotaSettings>> {
        let settings = sqlx::query_as!(
            QuotaSettings,
            "SELECT guild_id, max_active_count, max_overlap_count, max_hours_7d, max_hours_30d,
                    created_at, updated_at
             FROM quota_settings WHERE guild_id = ?",
            guild_id
        )
        .fetch_optional(&self.db)
        .await?;
        
        Ok(settings)
    }

    /// Update quota settings for a guild
    pub async fn update_quota_settings(
        &self,
        guild_id: i64,
        max_active_count: Option<i64>,
        max_overlap_count: Option<i64>,
        max_hours_7d: Option<i64>,
        max_hours_30d: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO quota_settings 
             (guild_id, max_active_count, max_overlap_count, max_hours_7d, max_hours_30d, updated_at)
             VALUES (?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(guild_id) DO UPDATE SET
                max_active_count = excluded.max_active_count,
                max_overlap_count = excluded.max_overlap_count,
                max_hours_7d = excluded.max_hours_7d,
                max_hours_30d = excluded.max_hours_30d,
                updated_at = CURRENT_TIMESTAMP",
            guild_id,
            max_active_count,
            max_overlap_count,
            max_hours_7d,
            max_hours_30d
        )
        .execute(&self.db)
        .await?;
        
        Ok(())
    }

    /// Get role overrides for a guild
    pub async fn get_role_overrides(&self, guild_id: i64) -> Result<Vec<QuotaRoleOverride>> {
        let overrides = sqlx::query_as!(
            QuotaRoleOverride,
            "SELECT guild_id, role_id, max_active_count, max_overlap_count, max_hours_7d, max_hours_30d,
                    created_at, updated_at
             FROM quota_role_overrides WHERE guild_id = ?",
            guild_id
        )
        .fetch_all(&self.db)
        .await?;
        
        Ok(overrides)
    }

    /// Add or update role override
    pub async fn update_role_override(
        &self,
        guild_id: i64,
        role_id: i64,
        max_active_count: Option<i64>,
        max_overlap_count: Option<i64>,
        max_hours_7d: Option<i64>,
        max_hours_30d: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO quota_role_overrides 
             (guild_id, role_id, max_active_count, max_overlap_count, max_hours_7d, max_hours_30d, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(guild_id, role_id) DO UPDATE SET
                max_active_count = excluded.max_active_count,
                max_overlap_count = excluded.max_overlap_count,
                max_hours_7d = excluded.max_hours_7d,
                max_hours_30d = excluded.max_hours_30d,
                updated_at = CURRENT_TIMESTAMP",
            guild_id,
            role_id,
            max_active_count,
            max_overlap_count,
            max_hours_7d,
            max_hours_30d
        )
        .execute(&self.db)
        .await?;
        
        Ok(())
    }

    /// Remove role override
    pub async fn remove_role_override(&self, guild_id: i64, role_id: i64) -> Result<()> {
        sqlx::query!(
            "DELETE FROM quota_role_overrides WHERE guild_id = ? AND role_id = ?",
            guild_id,
            role_id
        )
        .execute(&self.db)
        .await?;
        
        Ok(())
    }

    /// Calculate effective quota limits for a user based on their roles
    pub async fn get_effective_limits(
        &self,
        guild_id: i64,
        user_roles: &[i64],
    ) -> Result<EffectiveQuotaLimits> {
        // Get guild base settings
        let base_settings = self.get_quota_settings(guild_id).await?;
        
        // Start with base limits
        let mut effective = EffectiveQuotaLimits {
            max_active_count: base_settings.as_ref().and_then(|s| s.max_active_count),
            max_overlap_count: base_settings.as_ref().and_then(|s| s.max_overlap_count),
            max_hours_7d: base_settings.as_ref().and_then(|s| s.max_hours_7d),
            max_hours_30d: base_settings.as_ref().and_then(|s| s.max_hours_30d),
            max_duration_hours: None,
            min_lead_time_minutes: None,
            max_lead_time_days: None,
        };

        // Apply role overrides (most permissive wins)
        if !user_roles.is_empty() {
            for &role_id in user_roles {
                if let Some(override_rule) = sqlx::query_as!(
                    QuotaRoleOverride,
                    "SELECT guild_id, role_id, max_active_count, max_overlap_count, max_hours_7d, max_hours_30d,
                            created_at, updated_at
                     FROM quota_role_overrides WHERE guild_id = ? AND role_id = ?",
                    guild_id,
                    role_id
                )
                .fetch_optional(&self.db)
                .await?
                {
                    // Take the maximum value for each limit (more permissive)
                    effective.max_active_count = max_option(effective.max_active_count, override_rule.max_active_count);
                    effective.max_overlap_count = max_option(effective.max_overlap_count, override_rule.max_overlap_count);
                    effective.max_hours_7d = max_option(effective.max_hours_7d, override_rule.max_hours_7d);
                    effective.max_hours_30d = max_option(effective.max_hours_30d, override_rule.max_hours_30d);
                }
            }
        }

        Ok(effective)
    }

    /// Calculate effective quota limits for a user and equipment class
    /// This method combines guild settings, role overrides, and class-specific overrides
    /// The most permissive value wins for each dimension
    pub async fn get_effective_limits_with_class(
        &self,
        guild_id: i64,
        user_roles: &[i64],
        equipment_class_id: Option<i64>,
    ) -> Result<EffectiveQuotaLimits> {
        // Start with global/role effective limits
        let mut effective = self.get_effective_limits(guild_id, user_roles).await?;

        // Apply class-specific overrides if equipment has a class
        if let Some(class_id) = equipment_class_id {
            if let Some(class_override) = sqlx::query_as!(
                QuotaClassOverride,
                "SELECT guild_id, class_id, max_active_count, max_overlap_count, 
                 max_hours_7d, max_hours_30d, max_duration_hours, min_lead_time_minutes,
                 max_lead_time_days, created_at, updated_at
                 FROM quota_class_overrides 
                 WHERE guild_id = ? AND class_id = ?",
                guild_id,
                class_id
            )
            .fetch_optional(&self.db)
            .await?
            {
                // Take the maximum value for basic quota limits (more permissive)
                effective.max_active_count = max_option(effective.max_active_count, class_override.max_active_count);
                effective.max_overlap_count = max_option(effective.max_overlap_count, class_override.max_overlap_count);
                effective.max_hours_7d = max_option(effective.max_hours_7d, class_override.max_hours_7d);
                effective.max_hours_30d = max_option(effective.max_hours_30d, class_override.max_hours_30d);

                // For class-specific constraints (duration, lead time), use class values directly
                // These are class-specific requirements, not subject to "most permissive" logic
                effective.max_duration_hours = class_override.max_duration_hours;
                effective.min_lead_time_minutes = class_override.min_lead_time_minutes;
                effective.max_lead_time_days = class_override.max_lead_time_days;
            }
        }

        Ok(effective)
    }

    /// Check if user is within quota limits for a proposed reservation
    pub async fn validate_quota_limits(
        &self,
        guild_id: i64,
        user_id: i64,
        user_roles: &[i64],
        proposed_start: DateTime<Utc>,
        proposed_end: DateTime<Utc>,
        exclude_reservation_id: Option<i64>,
    ) -> Result<QuotaValidationResult> {
        let limits = self.get_effective_limits(guild_id, user_roles).await?;
        
        // If no limits are set, allow everything
        if limits.max_active_count.is_none() && 
           limits.max_overlap_count.is_none() && 
           limits.max_hours_7d.is_none() && 
           limits.max_hours_30d.is_none() {
            return Ok(QuotaValidationResult::Success);
        }

        let now = Utc::now();
        
        // Check active reservation count
        if let Some(max_active) = limits.max_active_count {
            let active_count = self.get_active_reservation_count(user_id, exclude_reservation_id).await?;
            if active_count >= max_active {
                return Ok(QuotaValidationResult::ExceededActiveCount {
                    current: active_count,
                    limit: max_active,
                });
            }
        }

        // Check simultaneous reservation count (overlapping with proposed time)
        if let Some(max_overlap) = limits.max_overlap_count {
            let overlap_count = self.get_overlapping_reservation_count(
                user_id,
                proposed_start,
                proposed_end,
                exclude_reservation_id,
            ).await?;
            
            if overlap_count >= max_overlap {
                return Ok(QuotaValidationResult::ExceededOverlapCount {
                    current: overlap_count,
                    limit: max_overlap,
                });
            }
        }

        // Check 7-day rolling window hours
        if let Some(max_hours_7d) = limits.max_hours_7d {
            let window_start = now - Duration::days(7);
            let current_hours = self.get_hours_in_window(
                user_id,
                window_start,
                now,
                exclude_reservation_id,
            ).await?;
            
            let proposed_hours = calculate_duration_hours(proposed_start, proposed_end);
            if current_hours + proposed_hours > max_hours_7d as f64 {
                return Ok(QuotaValidationResult::ExceededHours7d {
                    current: current_hours,
                    proposed: proposed_hours,
                    limit: max_hours_7d as f64,
                });
            }
        }

        // Check 30-day rolling window hours
        if let Some(max_hours_30d) = limits.max_hours_30d {
            let window_start = now - Duration::days(30);
            let current_hours = self.get_hours_in_window(
                user_id,
                window_start,
                now,
                exclude_reservation_id,
            ).await?;
            
            let proposed_hours = calculate_duration_hours(proposed_start, proposed_end);
            if current_hours + proposed_hours > max_hours_30d as f64 {
                return Ok(QuotaValidationResult::ExceededHours30d {
                    current: current_hours,
                    proposed: proposed_hours,
                    limit: max_hours_30d as f64,
                });
            }
        }

        Ok(QuotaValidationResult::Success)
    }

    /// Check if user is within quota limits including class-specific constraints
    pub async fn validate_quota_limits_with_class(
        &self,
        guild_id: i64,
        user_id: i64,
        user_roles: &[i64],
        equipment_class_id: Option<i64>,
        proposed_start: DateTime<Utc>,
        proposed_end: DateTime<Utc>,
        exclude_reservation_id: Option<i64>,
    ) -> Result<QuotaValidationResult> {
        let limits = self.get_effective_limits_with_class(guild_id, user_roles, equipment_class_id).await?;
        
        // First check all the regular quota limits (active count, overlap, time windows)
        let basic_result = self.validate_quota_limits(
            guild_id, 
            user_id, 
            user_roles, 
            proposed_start, 
            proposed_end, 
            exclude_reservation_id
        ).await?;
        
        if !basic_result.is_success() {
            return Ok(basic_result);
        }

        // Now check class-specific constraints
        let now = Utc::now();
        
        // Check maximum duration
        if let Some(max_duration_hours) = limits.max_duration_hours {
            let proposed_hours = calculate_duration_hours(proposed_start, proposed_end);
            if proposed_hours > max_duration_hours as f64 {
                return Ok(QuotaValidationResult::ExceededMaxDuration {
                    proposed_hours,
                    limit_hours: max_duration_hours,
                });
            }
        }

        // Check minimum lead time
        if let Some(min_lead_time_minutes) = limits.min_lead_time_minutes {
            let lead_time = proposed_start - now;
            let lead_minutes = lead_time.num_minutes();
            if lead_minutes < min_lead_time_minutes {
                return Ok(QuotaValidationResult::TooShortLeadTime {
                    proposed_minutes: lead_minutes,
                    min_minutes: min_lead_time_minutes,
                });
            }
        }

        // Check maximum lead time
        if let Some(max_lead_time_days) = limits.max_lead_time_days {
            let lead_time = proposed_start - now;
            let lead_days = lead_time.num_days();
            if lead_days > max_lead_time_days {
                return Ok(QuotaValidationResult::TooLongLeadTime {
                    proposed_days: lead_days,
                    max_days: max_lead_time_days,
                });
            }
        }

        Ok(QuotaValidationResult::Success)
    }

    /// Get count of active reservations for a user
    async fn get_active_reservation_count(
        &self,
        user_id: i64,
        exclude_reservation_id: Option<i64>,
    ) -> Result<i64> {
        let count = if let Some(exclude_id) = exclude_reservation_id {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed' AND returned_at IS NULL 
                 AND end_time > datetime('now') AND id != ?",
                user_id,
                exclude_id
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

        Ok(count.unwrap_or(0) as i64)
    }

    /// Get count of reservations that overlap with proposed time window
    async fn get_overlapping_reservation_count(
        &self,
        user_id: i64,
        proposed_start: DateTime<Utc>,
        proposed_end: DateTime<Utc>,
        exclude_reservation_id: Option<i64>,
    ) -> Result<i64> {
        let start_naive = proposed_start.naive_utc();
        let end_naive = proposed_end.naive_utc();
        
        let count = if let Some(exclude_id) = exclude_reservation_id {
            sqlx::query_scalar!(
                "SELECT COUNT(*) FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed' AND returned_at IS NULL
                 AND start_time < ? AND end_time > ? AND id != ?",
                user_id,
                end_naive,
                start_naive,
                exclude_id
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

        Ok(count.unwrap_or(0) as i64)
    }

    /// Calculate total hours used in a time window, accounting for returned reservations
    async fn get_hours_in_window(
        &self,
        user_id: i64,
        window_start: DateTime<Utc>,
        window_end: DateTime<Utc>,
        exclude_reservation_id: Option<i64>,
    ) -> Result<f64> {
        let start_naive = window_start.naive_utc();
        let end_naive = window_end.naive_utc();
        
        let reservations = if let Some(exclude_id) = exclude_reservation_id {
            sqlx::query!(
                "SELECT start_time, end_time, returned_at FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed'
                 AND start_time < ? AND end_time > ? AND id != ?",
                user_id,
                end_naive,
                start_naive,
                exclude_id
            )
            .fetch_all(&self.db)
            .await?
        } else {
            sqlx::query!(
                "SELECT start_time, end_time, returned_at FROM reservations 
                 WHERE user_id = ? AND status = 'Confirmed'
                 AND start_time < ? AND end_time > ?",
                user_id,
                end_naive,
                start_naive
            )
            .fetch_all(&self.db)
            .await?
        };

        let mut total_hours = 0.0;
        
        for reservation in reservations {
            let start_utc = DateTime::<Utc>::from_naive_utc_and_offset(reservation.start_time, Utc);
            let end_utc = DateTime::<Utc>::from_naive_utc_and_offset(reservation.end_time, Utc);
            
            // Use returned time if available, otherwise use reservation end time
            let actual_end = if let Some(returned_at_naive) = reservation.returned_at {
                DateTime::<Utc>::from_naive_utc_and_offset(returned_at_naive, Utc)
            } else {
                end_utc
            };
            
            // Calculate overlap with window
            let overlap_start = std::cmp::max(start_utc, window_start);
            let overlap_end = std::cmp::min(actual_end, window_end);
            
            if overlap_start < overlap_end {
                total_hours += calculate_duration_hours(overlap_start, overlap_end);
            }
        }

        Ok(total_hours)
    }

    /// Record a quota override by an admin
    pub async fn record_quota_override(
        &self,
        guild_id: i64,
        reservation_id: Option<i64>,
        user_id: i64,
        acted_by_user_id: i64,
        reason: Option<String>,
    ) -> Result<i64> {
        let result = sqlx::query!(
            "INSERT INTO quota_override_audits 
             (guild_id, reservation_id, user_id, acted_by_user_id, reason)
             VALUES (?, ?, ?, ?, ?)",
            guild_id,
            reservation_id,
            user_id,
            acted_by_user_id,
            reason
        )
        .execute(&self.db)
        .await?;

        Ok(result.last_insert_rowid())
    }

    /// Get recent quota overrides for a guild
    pub async fn get_recent_overrides(
        &self,
        guild_id: i64,
        limit: i64,
    ) -> Result<Vec<QuotaOverrideAudit>> {
        let rows = sqlx::query!(
            "SELECT id, guild_id, reservation_id, user_id, acted_by_user_id, reason, created_at_utc
             FROM quota_override_audits 
             WHERE guild_id = ?
             ORDER BY created_at_utc DESC
             LIMIT ?",
            guild_id,
            limit
        )
        .fetch_all(&self.db)
        .await?;

        let mut overrides = Vec::new();
        for row in rows {
            overrides.push(QuotaOverrideAudit {
                id: row.id,
                guild_id: row.guild_id,
                reservation_id: row.reservation_id,
                user_id: row.user_id,
                acted_by_user_id: row.acted_by_user_id,
                reason: row.reason,
                created_at_utc: DateTime::<Utc>::from_naive_utc_and_offset(row.created_at_utc, Utc),
            });
        }

        Ok(overrides)
    }
}

/// Result of quota validation
#[derive(Debug, Clone)]
pub enum QuotaValidationResult {
    Success,
    ExceededActiveCount { current: i64, limit: i64 },
    ExceededOverlapCount { current: i64, limit: i64 },
    ExceededHours7d { current: f64, proposed: f64, limit: f64 },
    ExceededHours30d { current: f64, proposed: f64, limit: f64 },
    // Class-specific validation errors
    ExceededMaxDuration { proposed_hours: f64, limit_hours: i64 },
    TooShortLeadTime { proposed_minutes: i64, min_minutes: i64 },
    TooLongLeadTime { proposed_days: i64, max_days: i64 },
}

impl QuotaValidationResult {
    pub fn is_success(&self) -> bool {
        matches!(self, QuotaValidationResult::Success)
    }

    pub fn error_message(&self) -> Option<String> {
        match self {
            QuotaValidationResult::Success => None,
            QuotaValidationResult::ExceededActiveCount { current, limit } => {
                Some(format!(
                    "❌ **Quota exceeded: Active reservations**\n\nYou have {} active reservations, but the limit is {}.\n\n💡 **Tip:** Return some equipment or wait for reservations to end before making new ones.",
                    current, limit
                ))
            }
            QuotaValidationResult::ExceededOverlapCount { current, limit } => {
                Some(format!(
                    "❌ **Quota exceeded: Simultaneous reservations**\n\nThis reservation would overlap with {} others, but the limit is {}.\n\n💡 **Tip:** Choose a different time slot or adjust your reservation times.",
                    current, limit
                ))
            }
            QuotaValidationResult::ExceededHours7d { current, proposed, limit } => {
                Some(format!(
                    "❌ **Quota exceeded: 7-day usage limit**\n\nYou've used {:.1} hours in the past 7 days. Adding {:.1} hours would exceed the {:.1} hour limit.\n\n💡 **Tip:** Try a shorter reservation or wait a few days.",
                    current, proposed, limit
                ))
            }
            QuotaValidationResult::ExceededHours30d { current, proposed, limit } => {
                Some(format!(
                    "❌ **Quota exceeded: 30-day usage limit**\n\nYou've used {:.1} hours in the past 30 days. Adding {:.1} hours would exceed the {:.1} hour limit.\n\n💡 **Tip:** Try a shorter reservation or wait for your usage to reset.",
                    current, proposed, limit
                ))
            }
            QuotaValidationResult::ExceededMaxDuration { proposed_hours, limit_hours } => {
                Some(format!(
                    "❌ **Class limit exceeded: Maximum duration**\n\nThis reservation is {:.1} hours long, but the maximum duration for this equipment class is {} hours.\n\n💡 **Tip:** Shorten your reservation or contact an admin for an exception.",
                    proposed_hours, limit_hours
                ))
            }
            QuotaValidationResult::TooShortLeadTime { proposed_minutes, min_minutes } => {
                Some(format!(
                    "❌ **Class limit exceeded: Minimum lead time**\n\nThis reservation is only {} minutes from now, but this equipment class requires at least {} minutes lead time.\n\n💡 **Tip:** Reserve further in advance or contact an admin for an emergency exception.",
                    proposed_minutes, min_minutes
                ))
            }
            QuotaValidationResult::TooLongLeadTime { proposed_days, max_days } => {
                Some(format!(
                    "❌ **Class limit exceeded: Maximum lead time**\n\nThis reservation is {} days from now, but this equipment class only allows reservations up to {} days in advance.\n\n💡 **Tip:** Wait to make your reservation closer to the date you need it.",
                    proposed_days, max_days
                ))
            }
        }
    }
}

/// Helper function to take the maximum of two optional values (None is treated as unlimited)
pub fn max_option(a: Option<i64>, b: Option<i64>) -> Option<i64> {
    match (a, b) {
        (None, _) => None,  // None means unlimited
        (_, None) => None,  // None means unlimited
        (Some(x), Some(y)) => Some(std::cmp::max(x, y)),
    }
}

/// Calculate duration in hours between two timestamps
pub fn calculate_duration_hours(start: DateTime<Utc>, end: DateTime<Utc>) -> f64 {
    let duration = end - start;
    duration.num_seconds() as f64 / 3600.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Duration};

    #[test]
    fn test_max_option() {
        assert_eq!(max_option(None, None), None);
        assert_eq!(max_option(Some(5), None), None);
        assert_eq!(max_option(None, Some(10)), None);
        assert_eq!(max_option(Some(5), Some(10)), Some(10));
        assert_eq!(max_option(Some(15), Some(10)), Some(15));
    }

    #[test]
    fn test_calculate_duration_hours() {
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 1, 1, 12, 30, 0).unwrap();
        
        assert_eq!(calculate_duration_hours(start, end), 2.5);
    }

    #[test]
    fn test_quota_validation_result_messages() {
        let result = QuotaValidationResult::ExceededActiveCount { current: 3, limit: 2 };
        assert!(result.error_message().unwrap().contains("3 active reservations"));
        assert!(result.error_message().unwrap().contains("limit is 2"));

        let success = QuotaValidationResult::Success;
        assert!(success.error_message().is_none());
        assert!(success.is_success());
    }
}