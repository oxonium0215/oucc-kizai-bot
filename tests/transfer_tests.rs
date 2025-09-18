use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use oucc_kizai_bot::models::*;

mod common;

/// Transfer state transitions and business rules
#[derive(Debug, Clone, PartialEq)]
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

/// Check if a state transition is valid
pub fn is_valid_transfer_transition(from: &TransferStatus, to: &TransferStatus) -> bool {
    match (from, to) {
        // From Pending
        (TransferStatus::Pending, TransferStatus::Accepted) => true,
        (TransferStatus::Pending, TransferStatus::Denied) => true,
        (TransferStatus::Pending, TransferStatus::Expired) => true,
        (TransferStatus::Pending, TransferStatus::Canceled) => true,

        // Terminal states cannot transition
        (TransferStatus::Accepted, _) => false,
        (TransferStatus::Denied, _) => false,
        (TransferStatus::Expired, _) => false,
        (TransferStatus::Canceled, _) => false,

        // Invalid transitions
        _ => false,
    }
}

/// Calculate transfer expiry time (3 hours from creation)
pub fn calculate_transfer_expiry(created_at: DateTime<Utc>) -> DateTime<Utc> {
    created_at + Duration::hours(3)
}

/// Check if transfer has expired
pub fn is_transfer_expired(expires_at: DateTime<Utc>, current_time: DateTime<Utc>) -> bool {
    current_time >= expires_at
}

/// Find pending transfer for reservation (business rule: only one pending at a time)
pub async fn find_pending_transfer(
    db: &sqlx::SqlitePool,
    reservation_id: i64,
) -> Result<Option<TransferRequest>> {
    let transfer = sqlx::query_as!(
        TransferRequest,
        "SELECT * FROM transfer_requests 
         WHERE reservation_id = ? AND status = 'Pending'
         ORDER BY created_at DESC
         LIMIT 1",
        reservation_id
    )
    .fetch_optional(db)
    .await?;

    Ok(transfer)
}

/// Create a new transfer request (ensures single pending invariant)
pub async fn create_transfer_request(
    db: &sqlx::SqlitePool,
    reservation_id: i64,
    from_user_id: i64,
    to_user_id: i64,
) -> Result<TransferRequest> {
    let mut tx = db.begin().await?;

    // Check for existing pending transfer
    let existing = sqlx::query!(
        "SELECT id FROM transfer_requests 
         WHERE reservation_id = ? AND status = 'Pending'",
        reservation_id
    )
    .fetch_optional(&mut *tx)
    .await?;

    if existing.is_some() {
        tx.rollback().await?;
        return Err(anyhow::anyhow!(
            "A pending transfer request already exists for this reservation"
        ));
    }

    let now = Utc::now();
    let expires_at = calculate_transfer_expiry(now);

    let result = sqlx::query!(
        "INSERT INTO transfer_requests 
         (reservation_id, from_user_id, to_user_id, requested_by_user_id, execute_at_utc, note, expires_at, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, NULL, NULL, ?, 'Pending', ?, ?) RETURNING id",
        reservation_id,
        from_user_id,
        to_user_id,
        from_user_id, // requested_by same as from for direct transfers
        expires_at,
        now,
        now
    )
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    // Fetch the created transfer request
    let transfer = sqlx::query_as!(
        TransferRequest,
        "SELECT * FROM transfer_requests WHERE id = ?",
        result.id
    )
    .fetch_one(db)
    .await?;

    Ok(transfer)
}

/// Update transfer status with validation
pub async fn update_transfer_status(
    db: &sqlx::SqlitePool,
    transfer_id: i64,
    new_status: TransferStatus,
) -> Result<()> {
    let mut tx = db.begin().await?;

    // Get current status
    let current = sqlx::query!(
        "SELECT status FROM transfer_requests WHERE id = ?",
        transfer_id
    )
    .fetch_one(&mut *tx)
    .await?;

    let current_status = TransferStatus::from(current.status);

    // Validate transition
    if !is_valid_transfer_transition(&current_status, &new_status) {
        tx.rollback().await?;
        return Err(anyhow::anyhow!(
            "Invalid transfer status transition from {:?} to {:?}",
            current_status,
            new_status
        ));
    }

    // Update status
    sqlx::query!(
        "UPDATE transfer_requests SET status = ?, updated_at = CURRENT_TIMESTAMP WHERE id = ?",
        String::from(new_status),
        transfer_id
    )
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;
    Ok(())
}

#[tokio::test]
async fn test_transfer_state_machine() {
    // Test valid transitions
    assert!(is_valid_transfer_transition(
        &TransferStatus::Pending,
        &TransferStatus::Accepted
    ));
    assert!(is_valid_transfer_transition(
        &TransferStatus::Pending,
        &TransferStatus::Denied
    ));
    assert!(is_valid_transfer_transition(
        &TransferStatus::Pending,
        &TransferStatus::Expired
    ));
    assert!(is_valid_transfer_transition(
        &TransferStatus::Pending,
        &TransferStatus::Canceled
    ));

    // Test invalid transitions from terminal states
    assert!(!is_valid_transfer_transition(
        &TransferStatus::Accepted,
        &TransferStatus::Denied
    ));
    assert!(!is_valid_transfer_transition(
        &TransferStatus::Denied,
        &TransferStatus::Accepted
    ));
    assert!(!is_valid_transfer_transition(
        &TransferStatus::Expired,
        &TransferStatus::Accepted
    ));
    assert!(!is_valid_transfer_transition(
        &TransferStatus::Canceled,
        &TransferStatus::Accepted
    ));

    // Test transitions to same state
    assert!(!is_valid_transfer_transition(
        &TransferStatus::Pending,
        &TransferStatus::Pending
    ));
    assert!(!is_valid_transfer_transition(
        &TransferStatus::Accepted,
        &TransferStatus::Accepted
    ));
}

#[test]
fn test_transfer_expiry_calculation() {
    let created_at = DateTime::parse_from_rfc3339("2024-01-01T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    let expires_at = calculate_transfer_expiry(created_at);

    // Should be 3 hours later
    assert_eq!(expires_at, created_at + Duration::hours(3));

    // Test expiry checking
    let current_time = created_at + Duration::hours(2);
    assert!(!is_transfer_expired(expires_at, current_time));

    let current_time = created_at + Duration::hours(3);
    assert!(is_transfer_expired(expires_at, current_time));

    let current_time = created_at + Duration::hours(4);
    assert!(is_transfer_expired(expires_at, current_time));
}

#[tokio::test]
async fn test_single_pending_transfer_invariant() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;

    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        12345,
        Utc::now(),
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;

    // Create first transfer request
    let transfer1 = create_transfer_request(&ctx.db, reservation.id, 12345, 67890).await?;

    assert_eq!(transfer1.status, "Pending");

    // Attempt to create second transfer request for same reservation
    let result = create_transfer_request(&ctx.db, reservation.id, 12345, 11111).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("pending transfer request already exists"));

    // After resolving first transfer, should be able to create new one
    update_transfer_status(&ctx.db, transfer1.id, TransferStatus::Denied).await?;

    let transfer2 = create_transfer_request(&ctx.db, reservation.id, 12345, 11111).await?;

    assert_eq!(transfer2.status, "Pending");

    Ok(())
}

#[tokio::test]
async fn test_transfer_status_validation() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;

    // Create a reservation and transfer
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        12345,
        Utc::now(),
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;

    let transfer = create_transfer_request(&ctx.db, reservation.id, 12345, 67890).await?;

    // Valid transition: Pending -> Accepted
    update_transfer_status(&ctx.db, transfer.id, TransferStatus::Accepted).await?;

    // Invalid transition: Accepted -> Denied (should fail)
    let result = update_transfer_status(&ctx.db, transfer.id, TransferStatus::Denied).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Invalid transfer status transition"));

    Ok(())
}

#[tokio::test]
async fn test_find_pending_transfer() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_, _, _, equipment) = common::create_test_setup(&ctx).await?;

    // Create a reservation
    let reservation = common::ReservationBuilder::new(
        equipment.id,
        12345,
        Utc::now(),
        Utc::now() + Duration::hours(2),
    )
    .build(&ctx.db)
    .await?;

    // No pending transfer initially
    let pending = find_pending_transfer(&ctx.db, reservation.id).await?;
    assert!(pending.is_none());

    // Create a transfer request
    let transfer = create_transfer_request(&ctx.db, reservation.id, 12345, 67890).await?;

    // Should find the pending transfer
    let pending = find_pending_transfer(&ctx.db, reservation.id).await?;
    assert!(pending.is_some());
    assert_eq!(pending.unwrap().id, transfer.id);

    // Resolve the transfer
    update_transfer_status(&ctx.db, transfer.id, TransferStatus::Accepted).await?;

    // Should not find pending transfer anymore
    let pending = find_pending_transfer(&ctx.db, reservation.id).await?;
    assert!(pending.is_none());

    Ok(())
}

#[test]
fn test_transfer_status_conversions() {
    // Test string to enum conversion
    assert_eq!(
        TransferStatus::from("Pending".to_string()),
        TransferStatus::Pending
    );
    assert_eq!(
        TransferStatus::from("Accepted".to_string()),
        TransferStatus::Accepted
    );
    assert_eq!(
        TransferStatus::from("Denied".to_string()),
        TransferStatus::Denied
    );
    assert_eq!(
        TransferStatus::from("Expired".to_string()),
        TransferStatus::Expired
    );
    assert_eq!(
        TransferStatus::from("Canceled".to_string()),
        TransferStatus::Canceled
    );
    assert_eq!(
        TransferStatus::from("Invalid".to_string()),
        TransferStatus::Pending
    ); // Default

    // Test enum to string conversion
    assert_eq!(String::from(TransferStatus::Pending), "Pending");
    assert_eq!(String::from(TransferStatus::Accepted), "Accepted");
    assert_eq!(String::from(TransferStatus::Denied), "Denied");
    assert_eq!(String::from(TransferStatus::Expired), "Expired");
    assert_eq!(String::from(TransferStatus::Canceled), "Canceled");
}
