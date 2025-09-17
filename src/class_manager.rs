use anyhow::Result;
use chrono::{DateTime, Utc, Duration};
use sqlx::SqlitePool;
use crate::models::{EquipmentClass, QuotaClassOverride, EffectiveQuotaLimits};

/// Helper for managing equipment classes and class-specific quotas
pub struct ClassManager {
    db: SqlitePool,
}

impl ClassManager {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Create a new equipment class
    pub async fn create_class(
        &self,
        guild_id: i64,
        name: &str,
        emoji: Option<&str>,
        description: Option<&str>,
    ) -> Result<i64> {
        let result = sqlx::query!(
            "INSERT INTO equipment_classes (guild_id, name, emoji, description) 
             VALUES (?, ?, ?, ?) RETURNING id",
            guild_id,
            name,
            emoji,
            description
        )
        .fetch_one(&self.db)
        .await?;

        Ok(result.id)
    }

    /// Get all equipment classes for a guild
    pub async fn get_classes_for_guild(&self, guild_id: i64) -> Result<Vec<EquipmentClass>> {
        let classes = sqlx::query_as!(
            EquipmentClass,
            "SELECT id, guild_id, name, emoji, description, created_at_utc 
             FROM equipment_classes 
             WHERE guild_id = ? 
             ORDER BY name",
            guild_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(classes)
    }

    /// Get a specific equipment class by ID
    pub async fn get_class(&self, class_id: i64) -> Result<Option<EquipmentClass>> {
        let class = sqlx::query_as!(
            EquipmentClass,
            "SELECT id, guild_id, name, emoji, description, created_at_utc 
             FROM equipment_classes 
             WHERE id = ?",
            class_id
        )
        .fetch_optional(&self.db)
        .await?;

        Ok(class)
    }

    /// Update equipment class information
    pub async fn update_class(
        &self,
        class_id: i64,
        name: Option<&str>,
        emoji: Option<Option<&str>>,
        description: Option<Option<&str>>,
    ) -> Result<()> {
        // Handle name update
        if let Some(n) = name {
            sqlx::query!(
                "UPDATE equipment_classes SET name = ? WHERE id = ?",
                n,
                class_id
            )
            .execute(&self.db)
            .await?;
        }

        // Handle emoji update
        if let Some(e) = emoji {
            sqlx::query!(
                "UPDATE equipment_classes SET emoji = ? WHERE id = ?",
                e,
                class_id
            )
            .execute(&self.db)
            .await?;
        }

        // Handle description update
        if let Some(d) = description {
            sqlx::query!(
                "UPDATE equipment_classes SET description = ? WHERE id = ?",
                d,
                class_id
            )
            .execute(&self.db)
            .await?;
        }

        Ok(())
    }

    /// Delete an equipment class (only if no equipment is assigned)
    pub async fn delete_class(&self, class_id: i64) -> Result<bool> {
        // Check if any equipment is assigned to this class
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*) FROM equipment WHERE class_id = ?",
            class_id
        )
        .fetch_one(&self.db)
        .await?;

        if count > 0 {
            return Ok(false); // Cannot delete class with assigned equipment
        }

        // Delete class-specific quota overrides first
        sqlx::query!(
            "DELETE FROM quota_class_overrides WHERE class_id = ?",
            class_id
        )
        .execute(&self.db)
        .await?;

        // Delete the class
        let result = sqlx::query!(
            "DELETE FROM equipment_classes WHERE id = ?",
            class_id
        )
        .execute(&self.db)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    /// Assign equipment to a class
    pub async fn assign_equipment_to_class(
        &self,
        equipment_id: i64,
        class_id: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            "UPDATE equipment SET class_id = ? WHERE id = ?",
            class_id,
            equipment_id
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Get equipment count by class for a guild
    pub async fn get_equipment_count_by_class(&self, guild_id: i64) -> Result<Vec<(Option<i64>, String, i64)>> {
        let counts = sqlx::query!(
            "SELECT e.class_id, COALESCE(ec.name, 'Unclassified') as class_name, COUNT(*) as count
             FROM equipment e
             LEFT JOIN equipment_classes ec ON e.class_id = ec.id
             WHERE e.guild_id = ?
             GROUP BY e.class_id, ec.name
             ORDER BY ec.name NULLS LAST",
            guild_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(counts.into_iter()
            .map(|row| (row.class_id, row.class_name, row.count))
            .collect())
    }

    /// Set class-specific quota overrides
    pub async fn set_class_quota_override(
        &self,
        guild_id: i64,
        class_id: i64,
        max_active_count: Option<i64>,
        max_overlap_count: Option<i64>,
        max_hours_7d: Option<i64>,
        max_hours_30d: Option<i64>,
        max_duration_hours: Option<i64>,
        min_lead_time_minutes: Option<i64>,
        max_lead_time_days: Option<i64>,
    ) -> Result<()> {
        sqlx::query!(
            "INSERT INTO quota_class_overrides 
             (guild_id, class_id, max_active_count, max_overlap_count, max_hours_7d, max_hours_30d,
              max_duration_hours, min_lead_time_minutes, max_lead_time_days)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (guild_id, class_id) DO UPDATE SET
                max_active_count = excluded.max_active_count,
                max_overlap_count = excluded.max_overlap_count,
                max_hours_7d = excluded.max_hours_7d,
                max_hours_30d = excluded.max_hours_30d,
                max_duration_hours = excluded.max_duration_hours,
                min_lead_time_minutes = excluded.min_lead_time_minutes,
                max_lead_time_days = excluded.max_lead_time_days,
                updated_at = CURRENT_TIMESTAMP",
            guild_id,
            class_id,
            max_active_count,
            max_overlap_count,
            max_hours_7d,
            max_hours_30d,
            max_duration_hours,
            min_lead_time_minutes,
            max_lead_time_days
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Get class-specific quota override
    pub async fn get_class_quota_override(
        &self,
        guild_id: i64,
        class_id: i64,
    ) -> Result<Option<QuotaClassOverride>> {
        let override_data = sqlx::query_as!(
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
        .await?;

        Ok(override_data)
    }

    /// Remove class-specific quota override
    pub async fn remove_class_quota_override(&self, guild_id: i64, class_id: i64) -> Result<()> {
        sqlx::query!(
            "DELETE FROM quota_class_overrides WHERE guild_id = ? AND class_id = ?",
            guild_id,
            class_id
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }
}