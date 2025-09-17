use anyhow::Result;
use chrono::{DateTime, Utc, TimeZone};
use oucc_kizai_bot::quota_validator::*;
use sqlx::SqlitePool;

mod common;

#[tokio::test]
async fn test_quota_validator_basic_functionality() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Apply quota migration
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let quota_validator = QuotaValidator::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let user_id = 111111111i64;
    let user_roles: Vec<i64> = vec![];
    
    // Test with no quotas configured - should succeed
    let start = Utc.with_ymd_and_hms(2024, 1, 15, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 15, 12, 0, 0).unwrap();
    
    let result = quota_validator.validate_reservation_quota(
        guild_id,
        user_id,
        &user_roles,
        start,
        end,
        None,
    ).await?;
    
    assert!(result.is_success());
    assert!(result.error_message().is_none());
    
    // Set up basic quotas
    sqlx::query!(
        "INSERT INTO quota_settings (guild_id, max_active_count, max_overlap_count) VALUES (?, ?, ?)",
        guild_id,
        2, // max active
        1  // max overlap
    )
    .execute(&ctx.db)
    .await?;
    
    // Test with quotas but no existing reservations - should succeed
    let result = quota_validator.validate_reservation_quota(
        guild_id,
        user_id,
        &user_roles,
        start,
        end,
        None,
    ).await?;
    
    assert!(result.is_success());
    
    Ok(())
}

#[tokio::test]
async fn test_quota_validation_result_error_messages() {
    let success = QuotaValidationResult::Success;
    assert!(success.is_success());
    assert!(success.error_message().is_none());
    
    let exceeded = QuotaValidationResult::Exceeded {
        message: "Test quota exceeded".to_string()
    };
    assert!(!exceeded.is_success());
    assert_eq!(exceeded.error_message(), Some("Test quota exceeded".to_string()));
}