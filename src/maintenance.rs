use anyhow::Result;
use chrono::{DateTime, Utc, NaiveDateTime};
use sqlx::SqlitePool;
use crate::models::MaintenanceWindow;

/// Helper functions for maintenance window operations
pub struct MaintenanceHelper {
    db: SqlitePool,
}

impl MaintenanceHelper {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Check if a time range conflicts with any active maintenance windows for equipment
    pub async fn check_maintenance_conflict(
        &self,
        equipment_id: i64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Option<MaintenanceWindow>> {
        // Convert to naive datetime for database query
        let start_naive = start_time.naive_utc();
        let end_naive = end_time.naive_utc();

        let maintenance_row = sqlx::query!(
            "SELECT id, equipment_id, start_utc, end_utc, reason, created_by_user_id, created_at_utc, canceled_at_utc, canceled_by_user_id
             FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND start_utc < ? AND end_utc > ?
             ORDER BY start_utc ASC
             LIMIT 1",
            equipment_id,
            end_naive,
            start_naive
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(row) = maintenance_row {
            let maintenance = MaintenanceWindow {
                id: row.id,
                equipment_id: row.equipment_id,
                start_utc: Self::naive_datetime_to_utc(row.start_utc),
                end_utc: Self::naive_datetime_to_utc(row.end_utc),
                reason: row.reason,
                created_by_user_id: row.created_by_user_id,
                created_at_utc: Self::naive_datetime_to_utc(row.created_at_utc),
                canceled_at_utc: row.canceled_at_utc.map(Self::naive_datetime_to_utc),
                canceled_by_user_id: row.canceled_by_user_id,
            };
            Ok(Some(maintenance))
        } else {
            Ok(None)
        }
    }

    /// Get current or next upcoming maintenance window for equipment
    pub async fn get_current_or_next_maintenance(
        &self,
        equipment_id: i64,
    ) -> Result<Option<MaintenanceWindow>> {
        let now_utc = Utc::now().naive_utc();

        let maintenance_row = sqlx::query!(
            "SELECT id, equipment_id, start_utc, end_utc, reason, created_by_user_id, created_at_utc, canceled_at_utc, canceled_by_user_id
             FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND end_utc > ?
             ORDER BY start_utc ASC
             LIMIT 1",
            equipment_id,
            now_utc
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(row) = maintenance_row {
            let maintenance = MaintenanceWindow {
                id: row.id,
                equipment_id: row.equipment_id,
                start_utc: Self::naive_datetime_to_utc(row.start_utc),
                end_utc: Self::naive_datetime_to_utc(row.end_utc),
                reason: row.reason,
                created_by_user_id: row.created_by_user_id,
                created_at_utc: Self::naive_datetime_to_utc(row.created_at_utc),
                canceled_at_utc: row.canceled_at_utc.map(Self::naive_datetime_to_utc),
                canceled_by_user_id: row.canceled_by_user_id,
            };
            Ok(Some(maintenance))
        } else {
            Ok(None)
        }
    }

    /// Create a new maintenance window with overlap checking
    pub async fn create_maintenance_window(
        &self,
        equipment_id: i64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        reason: Option<String>,
        created_by_user_id: i64,
    ) -> Result<i64, String> {
        // Check for overlaps with existing maintenance windows (disallow by default)
        let start_naive = start_time.naive_utc();
        let end_naive = end_time.naive_utc();

        let overlaps = sqlx::query!(
            "SELECT id, start_utc, end_utc FROM maintenance_windows 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             AND start_utc < ? AND end_utc > ?",
            equipment_id,
            end_naive,
            start_naive
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        if !overlaps.is_empty() {
            let overlap = &overlaps[0];
            let start_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(overlap.start_utc));
            let end_jst = crate::time::utc_to_jst_string(Self::naive_datetime_to_utc(overlap.end_utc));
            return Err(format!(
                "Maintenance window would overlap with existing maintenance from {} to {}",
                start_jst, end_jst
            ));
        }

        // Create the maintenance window
        let result = sqlx::query!(
            "INSERT INTO maintenance_windows (equipment_id, start_utc, end_utc, reason, created_by_user_id)
             VALUES (?, ?, ?, ?, ?)",
            equipment_id,
            start_naive,
            end_naive,
            reason,
            created_by_user_id
        )
        .execute(&self.db)
        .await
        .map_err(|e| format!("Failed to create maintenance window: {}", e))?;

        Ok(result.last_insert_rowid())
    }

    /// Cancel a maintenance window
    pub async fn cancel_maintenance_window(
        &self,
        maintenance_id: i64,
        canceled_by_user_id: i64,
    ) -> Result<(), String> {
        let now_utc = Utc::now().naive_utc();

        let result = sqlx::query!(
            "UPDATE maintenance_windows 
             SET canceled_at_utc = ?, canceled_by_user_id = ?
             WHERE id = ? AND canceled_at_utc IS NULL",
            now_utc,
            canceled_by_user_id,
            maintenance_id
        )
        .execute(&self.db)
        .await
        .map_err(|e| format!("Database error: {}", e))?;

        if result.rows_affected() == 0 {
            Err("Maintenance window not found or already canceled".to_string())
        } else {
            Ok(())
        }
    }

    /// Get affected reservations for a maintenance window
    pub async fn get_affected_reservations(
        &self,
        equipment_id: i64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
    ) -> Result<Vec<(i64, i64)>> { // (reservation_id, user_id)
        let start_naive = start_time.naive_utc();
        let end_naive = end_time.naive_utc();

        let reservations = sqlx::query!(
            "SELECT id, user_id FROM reservations 
             WHERE equipment_id = ? AND status = 'Confirmed' AND returned_at IS NULL
             AND start_time < ? AND end_time > ?",
            equipment_id,
            end_naive,
            start_naive
        )
        .fetch_all(&self.db)
        .await?;

        Ok(reservations.into_iter().map(|r| (r.id, r.user_id)).collect())
    }

    /// Helper function to convert NaiveDateTime to DateTime<Utc>
    fn naive_datetime_to_utc(naive: NaiveDateTime) -> DateTime<Utc> {
        DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
    }
}