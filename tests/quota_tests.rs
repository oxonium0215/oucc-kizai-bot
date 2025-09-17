use anyhow::Result;
use chrono::{DateTime, Utc, Duration, TimeZone};
use oucc_kizai_bot::quotas::*;
use sqlx::SqlitePool;

mod common;

#[tokio::test]
async fn test_quota_helper_basic_functionality() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Apply quota migration
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let quota_helper = QuotaHelper::new(ctx.db.clone());
    let guild_id = 123456789i64;
    
    // Test that no quota settings exist initially
    let settings = quota_helper.get_quota_settings(guild_id).await?;
    assert!(settings.is_none());
    
    // Test creating quota settings
    quota_helper.update_quota_settings(
        guild_id,
        Some(5),   // max_active_count
        Some(3),   // max_overlap_count  
        Some(168), // max_hours_7d (1 week)
        Some(720), // max_hours_30d (1 month)
    ).await?;
    
    // Test that settings were saved
    let settings = quota_helper.get_quota_settings(guild_id).await?;
    assert!(settings.is_some());
    let settings = settings.unwrap();
    assert_eq!(settings.max_active_count, Some(5));
    assert_eq!(settings.max_overlap_count, Some(3));
    assert_eq!(settings.max_hours_7d, Some(168));
    assert_eq!(settings.max_hours_30d, Some(720));
    
    Ok(())
}

#[tokio::test]
async fn test_quota_role_overrides() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let quota_helper = QuotaHelper::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let staff_role_id = 111111111i64;
    let guest_role_id = 222222222i64;
    
    // Set base quota settings
    quota_helper.update_quota_settings(guild_id, Some(3), Some(2), Some(50), Some(200)).await?;
    
    // Add role overrides - staff get higher limits
    quota_helper.update_role_override(
        guild_id,
        staff_role_id,
        Some(10), // Higher active limit
        Some(5),  // Higher overlap limit
        Some(100), // Higher 7d limit
        None,     // Use guild default for 30d
    ).await?;
    
    // Add guest role - lower limits
    quota_helper.update_role_override(
        guild_id,
        guest_role_id,
        Some(1), // Lower active limit
        Some(1), // Lower overlap limit
        Some(20), // Lower 7d limit
        Some(50), // Lower 30d limit
    ).await?;
    
    // Test effective limits for different roles
    
    // Base user (no roles) - gets guild defaults
    let base_limits = quota_helper.get_effective_limits(guild_id, &[]).await?;
    assert_eq!(base_limits.max_active_count, Some(3));
    assert_eq!(base_limits.max_overlap_count, Some(2));
    
    // Staff user - gets higher limits where specified
    let staff_limits = quota_helper.get_effective_limits(guild_id, &[staff_role_id]).await?;
    assert_eq!(staff_limits.max_active_count, Some(10));
    assert_eq!(staff_limits.max_overlap_count, Some(5));
    assert_eq!(staff_limits.max_hours_7d, Some(100));
    assert_eq!(staff_limits.max_hours_30d, Some(200)); // Guild default used
    
    // Guest user - gets lower limits
    let guest_limits = quota_helper.get_effective_limits(guild_id, &[guest_role_id]).await?;
    assert_eq!(guest_limits.max_active_count, Some(1));
    assert_eq!(guest_limits.max_overlap_count, Some(1));
    assert_eq!(guest_limits.max_hours_7d, Some(20));
    assert_eq!(guest_limits.max_hours_30d, Some(50));
    
    // User with both roles - gets most permissive (staff limits win)
    let mixed_limits = quota_helper.get_effective_limits(guild_id, &[staff_role_id, guest_role_id]).await?;
    assert_eq!(mixed_limits.max_active_count, Some(10)); // Staff wins
    assert_eq!(mixed_limits.max_overlap_count, Some(5));  // Staff wins
    assert_eq!(mixed_limits.max_hours_7d, Some(100));     // Staff wins
    assert_eq!(mixed_limits.max_hours_30d, Some(200));    // Guild default (most permissive)
    
    Ok(())
}

#[tokio::test]
async fn test_quota_override_auditing() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let quota_helper = QuotaHelper::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let user_id = 111111111i64;
    let admin_id = 222222222i64;
    let reservation_id = 333333333i64;
    
    // Record a quota override
    let audit_id = quota_helper.record_quota_override(
        guild_id,
        Some(reservation_id),
        user_id,
        admin_id,
        Some("Emergency equipment needed for critical project".to_string()),
    ).await?;
    
    assert!(audit_id > 0);
    
    // Retrieve recent overrides
    let overrides = quota_helper.get_recent_overrides(guild_id, 10).await?;
    assert_eq!(overrides.len(), 1);
    
    let override_record = &overrides[0];
    assert_eq!(override_record.guild_id, guild_id);
    assert_eq!(override_record.reservation_id, Some(reservation_id));
    assert_eq!(override_record.user_id, user_id);
    assert_eq!(override_record.acted_by_user_id, admin_id);
    assert!(override_record.reason.as_ref().unwrap().contains("Emergency"));
    
    Ok(())
}

#[tokio::test]
async fn test_quota_validation_result_messages() -> Result<()> {
    // Test quota validation result message formatting
    let success = QuotaValidationResult::Success;
    assert!(success.is_success());
    assert!(success.error_message().is_none());
    
    let active_exceeded = QuotaValidationResult::ExceededActiveCount { 
        current: 5, 
        limit: 3 
    };
    assert!(!active_exceeded.is_success());
    let message = active_exceeded.error_message().unwrap();
    assert!(message.contains("5 active reservations"));
    assert!(message.contains("limit is 3"));
    assert!(message.contains("Return some equipment"));
    
    let overlap_exceeded = QuotaValidationResult::ExceededOverlapCount { 
        current: 4, 
        limit: 2 
    };
    let message = overlap_exceeded.error_message().unwrap();
    assert!(message.contains("overlap with 4 others"));
    assert!(message.contains("limit is 2"));
    assert!(message.contains("different time slot"));
    
    let hours_7d_exceeded = QuotaValidationResult::ExceededHours7d { 
        current: 30.5, 
        proposed: 5.0, 
        limit: 32.0 
    };
    let message = hours_7d_exceeded.error_message().unwrap();
    assert!(message.contains("30.5 hours"));
    assert!(message.contains("5.0 hours"));
    assert!(message.contains("32.0 hour limit"));
    assert!(message.contains("7-day usage"));
    
    let hours_30d_exceeded = QuotaValidationResult::ExceededHours30d { 
        current: 120.0, 
        proposed: 10.0, 
        limit: 125.0 
    };
    let message = hours_30d_exceeded.error_message().unwrap();
    assert!(message.contains("120.0 hours"));
    assert!(message.contains("10.0 hours"));
    assert!(message.contains("125.0 hour limit"));
    assert!(message.contains("30-day usage"));
    
    Ok(())
}

#[tokio::test]
async fn test_quota_helper_functions() {
    use oucc_kizai_bot::quotas::{max_option, calculate_duration_hours};
    
    // Test max_option function
    assert_eq!(max_option(None, None), None);
    assert_eq!(max_option(Some(5), None), None);  // None means unlimited
    assert_eq!(max_option(None, Some(10)), None); // None means unlimited
    assert_eq!(max_option(Some(5), Some(10)), Some(10));
    assert_eq!(max_option(Some(15), Some(10)), Some(15));
    
    // Test calculate_duration_hours function
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 10, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 1, 12, 30, 0).unwrap();
    assert_eq!(calculate_duration_hours(start, end), 2.5);
    
    let start = Utc.with_ymd_and_hms(2024, 1, 1, 9, 0, 0).unwrap();
    let end = Utc.with_ymd_and_hms(2024, 1, 2, 9, 0, 0).unwrap();
    assert_eq!(calculate_duration_hours(start, end), 24.0);
}