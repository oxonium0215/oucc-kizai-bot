use anyhow::Result;
use chrono::{DateTime, Utc, Duration};
use sqlx::SqlitePool;
use tracing::{info, warn, error};
use serde_json::Value;

use crate::models::{WaitlistEntry, WaitlistOffer, Equipment, Reservation};
use crate::time::utc_to_jst_string;

/// Manager for waitlist operations including FIFO queue management and offer processing
pub struct WaitlistManager {
    db: SqlitePool,
}

/// Result of checking if a user can join a waitlist
#[derive(Debug)]
pub enum WaitlistJoinResult {
    Success(i64), // Returns waitlist entry ID
    AlreadyExists(i64), // User already has active entry for this equipment/window
    InvalidTimeWindow(String),
    DatabaseError(String),
}

/// Result of offering equipment to a waitlisted user
#[derive(Debug)]
pub struct WaitlistOfferResult {
    pub offer_id: i64,
    pub waitlist_entry: WaitlistEntry,
    pub offered_start: DateTime<Utc>,
    pub offered_end: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl WaitlistManager {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }

    /// Join a waitlist for a specific equipment and time window
    /// Returns WaitlistJoinResult indicating success or reason for failure
    pub async fn join_waitlist(
        &self,
        guild_id: i64,
        equipment_id: i64,
        user_id: i64,
        desired_start: DateTime<Utc>,
        desired_end: DateTime<Utc>,
    ) -> Result<WaitlistJoinResult> {
        // Validate time window
        if desired_start >= desired_end {
            return Ok(WaitlistJoinResult::InvalidTimeWindow(
                "Start time must be before end time".to_string()
            ));
        }

        if desired_start <= Utc::now() {
            return Ok(WaitlistJoinResult::InvalidTimeWindow(
                "Start time must be in the future".to_string()
            ));
        }

        // Check for existing active entry with same equipment and overlapping time window
        let existing = sqlx::query!(
            "SELECT id FROM waitlist_entries 
             WHERE guild_id = ? AND equipment_id = ? AND user_id = ? 
             AND canceled_at_utc IS NULL
             AND desired_start_utc < ? AND desired_end_utc > ?",
            guild_id,
            equipment_id,
            user_id,
            desired_end.naive_utc(),
            desired_start.naive_utc()
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(existing_entry) = existing {
            return Ok(WaitlistJoinResult::AlreadyExists(existing_entry.id.unwrap_or(0)));
        }

        // Create new waitlist entry
        let result = sqlx::query!(
            "INSERT INTO waitlist_entries 
             (guild_id, equipment_id, user_id, desired_start_utc, desired_end_utc) 
             VALUES (?, ?, ?, ?, ?) 
             RETURNING id",
            guild_id,
            equipment_id,
            user_id,
            desired_start.naive_utc(),
            desired_end.naive_utc()
        )
        .fetch_one(&self.db)
        .await?;

        let entry_id = result.id.unwrap_or(0);
        info!("User {} joined waitlist for equipment {} (entry ID: {})", user_id, equipment_id, entry_id);

        Ok(WaitlistJoinResult::Success(entry_id))
    }

    /// Leave/cancel a waitlist entry
    pub async fn leave_waitlist(&self, waitlist_id: i64, user_id: i64) -> Result<bool> {
        let rows_affected = sqlx::query!(
            "UPDATE waitlist_entries 
             SET canceled_at_utc = CURRENT_TIMESTAMP 
             WHERE id = ? AND user_id = ? AND canceled_at_utc IS NULL",
            waitlist_id,
            user_id
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("User {} left waitlist entry {}", user_id, waitlist_id);
            
            // Cancel any pending offers for this waitlist entry
            self.cancel_pending_offers(waitlist_id).await?;
            
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Admin function to cancel a waitlist entry
    pub async fn admin_cancel_waitlist(&self, waitlist_id: i64) -> Result<bool> {
        let rows_affected = sqlx::query!(
            "UPDATE waitlist_entries 
             SET canceled_at_utc = CURRENT_TIMESTAMP 
             WHERE id = ? AND canceled_at_utc IS NULL",
            waitlist_id
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("Admin canceled waitlist entry {}", waitlist_id);
            
            // Cancel any pending offers for this waitlist entry
            self.cancel_pending_offers(waitlist_id).await?;
            
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get active waitlist entries for a user
    pub async fn get_user_waitlist_entries(&self, guild_id: i64, user_id: i64) -> Result<Vec<WaitlistEntry>> {
        let entries = sqlx::query_as!(
            WaitlistEntry,
            "SELECT id, guild_id, equipment_id, user_id, 
                    desired_start_utc as 'desired_start_utc: DateTime<Utc>',
                    desired_end_utc as 'desired_end_utc: DateTime<Utc>',
                    created_at_utc as 'created_at_utc: DateTime<Utc>',
                    canceled_at_utc as 'canceled_at_utc: Option<DateTime<Utc>>'
             FROM waitlist_entries 
             WHERE guild_id = ? AND user_id = ? AND canceled_at_utc IS NULL
             ORDER BY created_at_utc ASC",
            guild_id,
            user_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(entries)
    }

    /// Get active waitlist entries for equipment (admin view)
    pub async fn get_equipment_waitlist_entries(&self, equipment_id: i64) -> Result<Vec<WaitlistEntry>> {
        let entries = sqlx::query_as!(
            WaitlistEntry,
            "SELECT id, guild_id, equipment_id, user_id, 
                    desired_start_utc as 'desired_start_utc: DateTime<Utc>',
                    desired_end_utc as 'desired_end_utc: DateTime<Utc>',
                    created_at_utc as 'created_at_utc: DateTime<Utc>',
                    canceled_at_utc as 'canceled_at_utc: Option<DateTime<Utc>>'
             FROM waitlist_entries 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL
             ORDER BY created_at_utc ASC",
            equipment_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(entries)
    }

    /// Count active waitlist entries for equipment
    pub async fn count_waitlist_entries(&self, equipment_id: i64) -> Result<i64> {
        let count = sqlx::query!(
            "SELECT COUNT(*) as count FROM waitlist_entries 
             WHERE equipment_id = ? AND canceled_at_utc IS NULL",
            equipment_id
        )
        .fetch_one(&self.db)
        .await?;

        Ok(count.count.unwrap_or(0) as i64)
    }

    /// Check if a time window is currently held by a waitlist offer
    /// This is used during conflict detection to enforce holds
    pub async fn check_waitlist_hold(
        &self,
        equipment_id: i64,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        exclude_user_id: Option<i64>,
    ) -> Result<Option<WaitlistOffer>> {
        let mut query = 
            "SELECT wo.id, wo.waitlist_id, wo.created_at_utc, wo.offer_expires_at_utc, 
                    wo.status, wo.reserved_reservation_id, wo.offered_window_start_utc, 
                    wo.offered_window_end_utc
             FROM waitlist_offers wo
             JOIN waitlist_entries we ON wo.waitlist_id = we.id
             WHERE we.equipment_id = ? AND wo.status = 'pending'
             AND wo.offer_expires_at_utc > CURRENT_TIMESTAMP
             AND wo.offered_window_start_utc < ? AND wo.offered_window_end_utc > ?".to_string();

        let params: Vec<Value> = if let Some(exclude_user) = exclude_user_id {
            query.push_str(" AND we.user_id != ?");
            vec![
                equipment_id.into(),
                end_time.naive_utc().to_string().into(),
                start_time.naive_utc().to_string().into(),
                exclude_user.into(),
            ]
        } else {
            vec![
                equipment_id.into(),
                end_time.naive_utc().to_string().into(),
                start_time.naive_utc().to_string().into(),
            ]
        };

        // Note: This is a simplified version. In a real implementation, we'd need to handle
        // the dynamic query parameters properly. For now, let's use a simpler approach.
        let hold = if exclude_user_id.is_some() {
            sqlx::query!(
                "SELECT wo.id, wo.waitlist_id, wo.created_at_utc, wo.offer_expires_at_utc, 
                        wo.status, wo.reserved_reservation_id, wo.offered_window_start_utc, 
                        wo.offered_window_end_utc
                 FROM waitlist_offers wo
                 JOIN waitlist_entries we ON wo.waitlist_id = we.id
                 WHERE we.equipment_id = ? AND wo.status = 'pending'
                 AND wo.offer_expires_at_utc > CURRENT_TIMESTAMP
                 AND wo.offered_window_start_utc < ? AND wo.offered_window_end_utc > ?
                 AND we.user_id != ?
                 LIMIT 1",
                equipment_id,
                end_time.naive_utc(),
                start_time.naive_utc(),
                exclude_user_id.unwrap()
            )
            .fetch_optional(&self.db)
            .await?
        } else {
            sqlx::query!(
                "SELECT wo.id, wo.waitlist_id, wo.created_at_utc, wo.offer_expires_at_utc, 
                        wo.status, wo.reserved_reservation_id, wo.offered_window_start_utc, 
                        wo.offered_window_end_utc
                 FROM waitlist_offers wo
                 JOIN waitlist_entries we ON wo.waitlist_id = we.id
                 WHERE we.equipment_id = ? AND wo.status = 'pending'
                 AND wo.offer_expires_at_utc > CURRENT_TIMESTAMP
                 AND wo.offered_window_start_utc < ? AND wo.offered_window_end_utc > ?
                 LIMIT 1",
                equipment_id,
                end_time.naive_utc(),
                start_time.naive_utc()
            )
            .fetch_optional(&self.db)
            .await?
        };

        if let Some(row) = hold {
            Ok(Some(WaitlistOffer {
                id: row.id.unwrap_or(0),
                waitlist_id: row.waitlist_id,
                created_at_utc: Self::naive_datetime_to_utc(row.created_at_utc.unwrap_or_default()),
                offer_expires_at_utc: Self::naive_datetime_to_utc(row.offer_expires_at_utc.unwrap_or_default()),
                status: row.status.unwrap_or_default(),
                reserved_reservation_id: row.reserved_reservation_id,
                offered_window_start_utc: Self::naive_datetime_to_utc(row.offered_window_start_utc.unwrap_or_default()),
                offered_window_end_utc: Self::naive_datetime_to_utc(row.offered_window_end_utc.unwrap_or_default()),
            }))
        } else {
            Ok(None)
        }
    }

    /// Cancel pending offers for a waitlist entry
    async fn cancel_pending_offers(&self, waitlist_id: i64) -> Result<()> {
        sqlx::query!(
            "UPDATE waitlist_offers 
             SET status = 'expired' 
             WHERE waitlist_id = ? AND status = 'pending'",
            waitlist_id
        )
        .execute(&self.db)
        .await?;

        Ok(())
    }

    /// Helper function to convert NaiveDateTime to DateTime<Utc>
    fn naive_datetime_to_utc(naive: chrono::NaiveDateTime) -> chrono::DateTime<chrono::Utc> {
        DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc)
    }
}

/// Helper functions for waitlist offer management
impl WaitlistManager {
    /// Create an offer for the next person in the waitlist queue
    /// This is called when equipment becomes available due to cancellation or return
    pub async fn create_offer_for_available_window(
        &self,
        equipment_id: i64,
        available_start: DateTime<Utc>,
        available_end: DateTime<Utc>,
        guild_id: i64,
    ) -> Result<Option<WaitlistOfferResult>> {
        // Find the next waitlist entry in FIFO order that fits in the available window
        let next_entry = sqlx::query!(
            "SELECT id, user_id, desired_start_utc, desired_end_utc, created_at_utc
             FROM waitlist_entries 
             WHERE equipment_id = ? AND guild_id = ? AND canceled_at_utc IS NULL
             AND desired_start_utc >= ? AND desired_end_utc <= ?
             ORDER BY created_at_utc ASC
             LIMIT 1",
            equipment_id,
            guild_id,
            available_start.naive_utc(),
            available_end.naive_utc()
        )
        .fetch_optional(&self.db)
        .await?;

        if let Some(entry_row) = next_entry {
            // Get guild settings for offer hold duration
            let guild_settings = sqlx::query!(
                "SELECT offer_hold_minutes FROM guilds WHERE id = ?",
                guild_id
            )
            .fetch_one(&self.db)
            .await?;

            let hold_minutes = guild_settings.offer_hold_minutes.unwrap_or(15);
            let expires_at = Utc::now() + Duration::minutes(hold_minutes);

            // Create the offer
            let offer_result = sqlx::query!(
                "INSERT INTO waitlist_offers 
                 (waitlist_id, offer_expires_at_utc, offered_window_start_utc, offered_window_end_utc)
                 VALUES (?, ?, ?, ?)
                 RETURNING id",
                entry_row.id,
                expires_at.naive_utc(),
                available_start.naive_utc(),
                available_end.naive_utc()
            )
            .fetch_one(&self.db)
            .await?;

            let offer_id = offer_result.id.unwrap_or(0);

            info!("Created offer {} for waitlist entry {} (user {})", 
                  offer_id, entry_row.id.unwrap_or(0), entry_row.user_id);

            let waitlist_entry = WaitlistEntry {
                id: entry_row.id.unwrap_or(0),
                guild_id,
                equipment_id,
                user_id: entry_row.user_id,
                desired_start_utc: Self::naive_datetime_to_utc(entry_row.desired_start_utc.unwrap_or_default()),
                desired_end_utc: Self::naive_datetime_to_utc(entry_row.desired_end_utc.unwrap_or_default()),
                created_at_utc: Self::naive_datetime_to_utc(entry_row.created_at_utc.unwrap_or_default()),
                canceled_at_utc: None,
            };

            Ok(Some(WaitlistOfferResult {
                offer_id,
                waitlist_entry,
                offered_start: available_start,
                offered_end: available_end,
                expires_at,
            }))
        } else {
            Ok(None)
        }
    }

    /// Accept a waitlist offer and create the reservation
    pub async fn accept_offer(&self, offer_id: i64, user_id: i64) -> Result<Option<i64>> {
        let mut tx = self.db.begin().await?;

        // Get and validate the offer
        let offer = sqlx::query!(
            "SELECT wo.id, wo.waitlist_id, wo.status, wo.offer_expires_at_utc,
                    wo.offered_window_start_utc, wo.offered_window_end_utc,
                    we.user_id, we.equipment_id, we.guild_id
             FROM waitlist_offers wo
             JOIN waitlist_entries we ON wo.waitlist_id = we.id
             WHERE wo.id = ? AND wo.status = 'pending'",
            offer_id
        )
        .fetch_optional(&mut *tx)
        .await?;

        let offer = match offer {
            Some(o) => o,
            None => {
                // Offer doesn't exist or is no longer pending
                return Ok(None);
            }
        };

        // Verify the user owns this offer
        if offer.user_id != user_id {
            return Ok(None);
        }

        // Check if offer has expired
        let expires_at = Self::naive_datetime_to_utc(offer.offer_expires_at_utc.unwrap_or_default());
        if expires_at <= Utc::now() {
            // Mark as expired
            sqlx::query!(
                "UPDATE waitlist_offers SET status = 'expired' WHERE id = ?",
                offer_id
            )
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            return Ok(None);
        }

        // Create the reservation
        let reservation_result = sqlx::query!(
            "INSERT INTO reservations 
             (equipment_id, user_id, start_time, end_time, status)
             VALUES (?, ?, ?, ?, 'Confirmed')
             RETURNING id",
            offer.equipment_id,
            user_id,
            offer.offered_window_start_utc,
            offer.offered_window_end_utc
        )
        .execute(&mut *tx)
        .await?;

        let reservation_id = reservation_result.last_insert_rowid();

        // Mark offer as accepted and link to reservation
        sqlx::query!(
            "UPDATE waitlist_offers 
             SET status = 'accepted', reserved_reservation_id = ? 
             WHERE id = ?",
            reservation_id,
            offer_id
        )
        .execute(&mut *tx)
        .await?;

        // Mark the waitlist entry as fulfilled (canceled)
        sqlx::query!(
            "UPDATE waitlist_entries 
             SET canceled_at_utc = CURRENT_TIMESTAMP 
             WHERE id = ?",
            offer.waitlist_id
        )
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        info!("User {} accepted offer {} and created reservation {}", 
              user_id, offer_id, reservation_id);

        Ok(Some(reservation_id))
    }

    /// Decline a waitlist offer
    pub async fn decline_offer(&self, offer_id: i64, user_id: i64) -> Result<bool> {
        let rows_affected = sqlx::query!(
            "UPDATE waitlist_offers wo
             SET status = 'declined'
             FROM waitlist_entries we
             WHERE wo.id = ? AND wo.waitlist_id = we.id AND we.user_id = ? 
             AND wo.status = 'pending'",
            offer_id,
            user_id
        )
        .execute(&self.db)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("User {} declined offer {}", user_id, offer_id);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Get pending offers for a user
    pub async fn get_pending_offers(&self, guild_id: i64, user_id: i64) -> Result<Vec<WaitlistOffer>> {
        let offers = sqlx::query_as!(
            WaitlistOffer,
            "SELECT wo.id, wo.waitlist_id, 
                    wo.created_at_utc as 'created_at_utc: DateTime<Utc>',
                    wo.offer_expires_at_utc as 'offer_expires_at_utc: DateTime<Utc>',
                    wo.status, wo.reserved_reservation_id,
                    wo.offered_window_start_utc as 'offered_window_start_utc: DateTime<Utc>',
                    wo.offered_window_end_utc as 'offered_window_end_utc: DateTime<Utc>'
             FROM waitlist_offers wo
             JOIN waitlist_entries we ON wo.waitlist_id = we.id
             WHERE we.guild_id = ? AND we.user_id = ? AND wo.status = 'pending'
             AND wo.offer_expires_at_utc > CURRENT_TIMESTAMP
             ORDER BY wo.created_at_utc ASC",
            guild_id,
            user_id
        )
        .fetch_all(&self.db)
        .await?;

        Ok(offers)
    }

    /// Process expired offers (called by scheduler)
    pub async fn process_expired_offers(&self) -> Result<Vec<i64>> {
        let expired_offers = sqlx::query!(
            "SELECT id FROM waitlist_offers 
             WHERE status = 'pending' AND offer_expires_at_utc <= CURRENT_TIMESTAMP"
        )
        .fetch_all(&self.db)
        .await?;

        let expired_ids: Vec<i64> = expired_offers.into_iter()
            .map(|o| o.id.unwrap_or(0))
            .collect();

        if !expired_ids.is_empty() {
            // Mark all as expired
            for &offer_id in &expired_ids {
                sqlx::query!(
                    "UPDATE waitlist_offers SET status = 'expired' WHERE id = ?",
                    offer_id
                )
                .execute(&self.db)
                .await?;
            }

            info!("Processed {} expired waitlist offers", expired_ids.len());
        }

        Ok(expired_ids)
    }
}