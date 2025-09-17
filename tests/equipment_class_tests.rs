use anyhow::Result;
use oucc_kizai_bot::models::{EquipmentClass, QuotaClassOverride, EffectiveQuotaLimits};
use oucc_kizai_bot::class_manager::ClassManager;
use oucc_kizai_bot::quotas::QuotaHelper;

mod common;

#[tokio::test]
async fn test_equipment_class_creation() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    
    // Apply migrations including our new equipment classes migration
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let class_manager = ClassManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    
    // Test creating a new equipment class
    let class_id = class_manager.create_class(
        guild_id,
        "Cameras",
        Some("📷"),
        Some("Professional camera equipment")
    ).await?;
    
    assert!(class_id > 0);
    
    // Test retrieving the created class
    let class = class_manager.get_class(class_id).await?;
    assert!(class.is_some());
    
    let class = class.unwrap();
    assert_eq!(class.name, "Cameras");
    assert_eq!(class.emoji, Some("📷".to_string()));
    assert_eq!(class.description, Some("Professional camera equipment".to_string()));
    
    Ok(())
}

#[tokio::test]
async fn test_class_quota_overrides() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let class_manager = ClassManager::new(ctx.db.clone());
    let quota_helper = QuotaHelper::new(ctx.db.clone());
    let guild_id = 123456789i64;
    
    // Create a class
    let class_id = class_manager.create_class(
        guild_id,
        "Laptops",
        Some("💻"),
        None
    ).await?;
    
    // Set class-specific quota overrides
    class_manager.set_class_quota_override(
        guild_id,
        class_id,
        Some(2),    // max_active_count
        Some(1),    // max_overlap_count  
        Some(50),   // max_hours_7d
        Some(200),  // max_hours_30d
        Some(24),   // max_duration_hours
        Some(60),   // min_lead_time_minutes (1 hour)
        Some(30),   // max_lead_time_days
    ).await?;
    
    // Test retrieving the override
    let override_data = class_manager.get_class_quota_override(guild_id, class_id).await?;
    assert!(override_data.is_some());
    
    let override_data = override_data.unwrap();
    assert_eq!(override_data.max_active_count, Some(2));
    assert_eq!(override_data.max_duration_hours, Some(24));
    assert_eq!(override_data.min_lead_time_minutes, Some(60));
    
    Ok(())
}

#[tokio::test]
async fn test_effective_limits_with_class() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let class_manager = ClassManager::new(ctx.db.clone());
    let quota_helper = QuotaHelper::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let role_id = 111111111i64;
    
    // Set base guild quota settings
    quota_helper.update_quota_settings(
        guild_id,
        Some(5),   // max_active_count
        Some(3),   // max_overlap_count
        Some(100), // max_hours_7d
        Some(400), // max_hours_30d
    ).await?;
    
    // Create a class with more restrictive limits
    let class_id = class_manager.create_class(
        guild_id,
        "Restricted Equipment",
        Some("🔒"),
        None
    ).await?;
    
    class_manager.set_class_quota_override(
        guild_id,
        class_id,
        Some(1),   // max_active_count (more restrictive than guild)
        Some(1),   // max_overlap_count (more restrictive than guild)
        Some(20),  // max_hours_7d (more restrictive than guild)
        Some(80),  // max_hours_30d (more restrictive than guild)
        Some(4),   // max_duration_hours
        Some(120), // min_lead_time_minutes (2 hours)
        Some(14),  // max_lead_time_days
    ).await?;
    
    // Test effective limits with class - should take most permissive values for basic limits
    let limits = quota_helper.get_effective_limits_with_class(
        guild_id,
        &[role_id],
        Some(class_id)
    ).await?;
    
    // Guild limits should win for basic quotas (more permissive)
    assert_eq!(limits.max_active_count, Some(5));  // Guild setting wins
    assert_eq!(limits.max_overlap_count, Some(3)); // Guild setting wins
    assert_eq!(limits.max_hours_7d, Some(100));    // Guild setting wins
    assert_eq!(limits.max_hours_30d, Some(400));   // Guild setting wins
    
    // Class-specific constraints should be present
    assert_eq!(limits.max_duration_hours, Some(4));
    assert_eq!(limits.min_lead_time_minutes, Some(120));
    assert_eq!(limits.max_lead_time_days, Some(14));
    
    Ok(())
}