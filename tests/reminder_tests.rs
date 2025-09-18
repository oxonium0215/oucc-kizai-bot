use anyhow::Result;
use chrono::Duration;
use oucc_kizai_bot::jobs::JobWorker;
use oucc_kizai_bot::traits::Clock;

mod common;

/// Test basic reminder job scheduling
#[tokio::test]
async fn test_reminder_job_creation() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;

    // Create a reservation
    let user_id = 12345i64;
    let reservation_start = ctx.clock.now_utc() + Duration::hours(1);
    let reservation_end = reservation_start + Duration::hours(2);

    let reservation =
        common::ReservationBuilder::new(equipment.id, user_id, reservation_start, reservation_end)
            .build(&ctx.db)
            .await?;

    // Schedule reminder jobs
    JobWorker::schedule_reservation_reminders(
        &ctx.db,
        reservation.id,
        reservation_start,
        reservation_end,
        guild.id,
    )
    .await?;

    // Verify jobs were created
    let job_count = sqlx::query!("SELECT COUNT(*) as count FROM jobs WHERE job_type = 'reminder'")
        .fetch_one(&ctx.db)
        .await?
        .count;

    // Should have pre-start, start, and pre-end reminders
    assert_eq!(job_count, 3);

    // Verify job payloads contain correct types
    let jobs =
        sqlx::query!("SELECT payload FROM jobs WHERE job_type = 'reminder' ORDER BY scheduled_for")
            .fetch_all(&ctx.db)
            .await?;

    assert!(jobs[0].payload.contains("pre_start"));
    assert!(jobs[1].payload.contains("\"type\":\"start\""));
    assert!(jobs[2].payload.contains("pre_end"));

    Ok(())
}

/// Test overdue reminder scheduling
#[tokio::test]
async fn test_overdue_reminder_scheduling() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;

    // Create a past reservation that should be overdue
    let user_id = 12345i64;
    let reservation_start = ctx.clock.now_utc() - Duration::hours(3);
    let reservation_end = ctx.clock.now_utc() - Duration::hours(1);

    let reservation =
        common::ReservationBuilder::new(equipment.id, user_id, reservation_start, reservation_end)
            .build(&ctx.db)
            .await?;

    // Schedule overdue reminders
    JobWorker::schedule_overdue_reminders(&ctx.db, reservation.id, reservation_end, guild.id)
        .await?;

    // Verify overdue jobs were created
    let overdue_job_count = sqlx::query!(
        "SELECT COUNT(*) as count FROM jobs WHERE job_type = 'reminder' AND payload LIKE '%return_delay%'"
    )
    .fetch_one(&ctx.db)
    .await?
    .count;

    // Should have multiple overdue reminders based on guild settings (default max 3)
    assert!(overdue_job_count > 0);
    assert!(overdue_job_count <= 3);

    Ok(())
}

/// Test reminder cancellation
#[tokio::test]
async fn test_cancel_reminders_on_return() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;

    let user_id = 12345i64;
    let reservation_start = ctx.clock.now_utc() + Duration::hours(1);
    let reservation_end = reservation_start + Duration::hours(2);

    let reservation =
        common::ReservationBuilder::new(equipment.id, user_id, reservation_start, reservation_end)
            .build(&ctx.db)
            .await?;

    // Schedule reminders
    JobWorker::schedule_reservation_reminders(
        &ctx.db,
        reservation.id,
        reservation_start,
        reservation_end,
        guild.id,
    )
    .await?;

    // Verify jobs exist
    let pending_jobs_before = sqlx::query!(
        "SELECT COUNT(*) as count FROM jobs WHERE status = 'Pending' AND JSON_EXTRACT(payload, '$.reservation_id') = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?
    .count;

    assert!(pending_jobs_before > 0);

    // Cancel reminders (simulate return)
    JobWorker::cancel_reservation_reminders(&ctx.db, reservation.id).await?;

    // Verify jobs were cancelled
    let pending_jobs_after = sqlx::query!(
        "SELECT COUNT(*) as count FROM jobs WHERE status = 'Pending' AND JSON_EXTRACT(payload, '$.reservation_id') = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?
    .count;

    let cancelled_jobs = sqlx::query!(
        "SELECT COUNT(*) as count FROM jobs WHERE status = 'Cancelled' AND JSON_EXTRACT(payload, '$.reservation_id') = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?
    .count;

    assert_eq!(pending_jobs_after, 0);
    assert_eq!(cancelled_jobs, pending_jobs_before);

    Ok(())
}

/// Test sent_reminders table functionality
#[tokio::test]
async fn test_sent_reminders_table() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    let (_guild, _tag, _location, equipment) = common::create_test_setup(&ctx).await?;

    let user_id = 12345i64;
    let reservation_start = ctx.clock.now_utc();
    let reservation_end = reservation_start + Duration::hours(2);

    let reservation =
        common::ReservationBuilder::new(equipment.id, user_id, reservation_start, reservation_end)
            .build(&ctx.db)
            .await?;

    // Insert a sent reminder manually
    let now = ctx.clock.now_utc();
    sqlx::query!(
        "INSERT INTO sent_reminders (reservation_id, kind, sent_at_utc, delivery_method)
         VALUES (?, 'PRE_END', ?, 'DM')",
        reservation.id,
        now
    )
    .execute(&ctx.db)
    .await?;

    // Verify it was inserted
    let reminder_count = sqlx::query!(
        "SELECT COUNT(*) as count FROM sent_reminders WHERE reservation_id = ?",
        reservation.id
    )
    .fetch_one(&ctx.db)
    .await?
    .count;

    assert_eq!(reminder_count, 1);

    // Test unique constraint (same reservation_id + kind should fail)
    let now2 = ctx.clock.now_utc();
    let result = sqlx::query!(
        "INSERT INTO sent_reminders (reservation_id, kind, sent_at_utc, delivery_method)
         VALUES (?, 'PRE_END', ?, 'DM')",
        reservation.id,
        now2
    )
    .execute(&ctx.db)
    .await;

    // Should fail due to unique constraint
    assert!(result.is_err());

    Ok(())
}
