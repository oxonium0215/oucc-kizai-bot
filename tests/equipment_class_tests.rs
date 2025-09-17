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
async fn test_equipment_class_display_integration() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let class_manager = ClassManager::new(ctx.db.clone());
    let guild_id = 123456789i64;
    
    // Create a guild record first
    sqlx::query!(
        "INSERT INTO guilds (id) VALUES (?)",
        guild_id
    )
    .execute(&ctx.db)
    .await?;
    
    // Create an equipment class
    let class_id = class_manager.create_class(
        guild_id,
        "Professional Cameras",
        Some("📷"),
        Some("High-end camera equipment requiring special handling")
    ).await?;
    
    // Create a tag for categorization
    sqlx::query!(
        "INSERT INTO tags (guild_id, name, sort_order) VALUES (?, ?, ?)",
        guild_id,
        "Photography",
        1
    )
    .execute(&ctx.db)
    .await?;
    
    let tag_id = ctx.db.last_insert_rowid();
    
    // Create equipment with the class assigned
    sqlx::query!(
        "INSERT INTO equipment (guild_id, tag_id, class_id, name, status) 
         VALUES (?, ?, ?, ?, ?)",
        guild_id,
        tag_id,
        class_id,
        "Canon EOS R5",
        "Available"
    )
    .execute(&ctx.db)
    .await?;
    
    let equipment_id = ctx.db.last_insert_rowid();
    
    // Verify that the equipment was created with the correct class
    let equipment = sqlx::query!(
        "SELECT id, guild_id, tag_id, class_id, name, status, 
         current_location, unavailable_reason, default_return_location, 
         message_id, created_at, updated_at
         FROM equipment WHERE id = ?",
        equipment_id
    )
    .fetch_one(&ctx.db)
    .await?;
    
    assert_eq!(equipment.class_id, Some(class_id));
    assert_eq!(equipment.name, "Canon EOS R5");
    
    // Verify that we can retrieve the class information
    let retrieved_class = class_manager.get_class(class_id).await?;
    assert!(retrieved_class.is_some());
    
    let class = retrieved_class.unwrap();
    assert_eq!(class.name, "Professional Cameras");
    assert_eq!(class.emoji, Some("📷".to_string()));
    
    Ok(())
}

#[tokio::test]
async fn test_class_specific_quota_validation() -> Result<()> {
    let ctx = common::TestContext::new().await?;
    sqlx::migrate!("./migrations").run(&ctx.db).await?;
    
    let class_manager = ClassManager::new(ctx.db.clone());
    let quota_helper = QuotaHelper::new(ctx.db.clone());
    let guild_id = 123456789i64;
    let user_id = 987654321i64;
    
    // Create a class with specific constraints  
    let class_id = class_manager.create_class(
        guild_id,
        "Time-Limited Equipment",
        Some("⏰"),
        None
    ).await?;
    
    // Set class constraints: max 2 hours duration, min 1 hour lead time, max 7 days advance
    class_manager.set_class_quota_override(
        guild_id,
        class_id,
        None,      // no active count limit
        None,      // no overlap limit  
        None,      // no 7d hours limit
        None,      // no 30d hours limit
        Some(2),   // max_duration_hours: 2 hours max
        Some(60),  // min_lead_time_minutes: 1 hour min
        Some(7),   // max_lead_time_days: max 7 days advance
    ).await?;
    
    use chrono::{Utc, Duration};
    let now = Utc::now();
    
    // Test 1: Duration too long (3 hours when max is 2)
    let start_time = now + Duration::hours(2); // 2 hours from now (valid lead time)
    let end_time = start_time + Duration::hours(3); // 3 hours duration (too long)
    
    let result = quota_helper.validate_quota_limits_with_class(
        guild_id,
        user_id,
        &[], // no roles
        Some(class_id),
        start_time,
        end_time,
        None
    ).await?;
    
    assert!(!result.is_success());
    assert!(result.error_message().unwrap().contains("Maximum duration"));
    
    // Test 2: Lead time too short (30 minutes when min is 60)
    let start_time = now + Duration::minutes(30); // 30 minutes from now (too short)
    let end_time = start_time + Duration::hours(1); // 1 hour duration (valid)
    
    let result = quota_helper.validate_quota_limits_with_class(
        guild_id,
        user_id,
        &[],
        Some(class_id),
        start_time,
        end_time,
        None
    ).await?;
    
    assert!(!result.is_success());
    assert!(result.error_message().unwrap().contains("Minimum lead time"));
    
    // Test 3: Lead time too long (10 days when max is 7)
    let start_time = now + Duration::days(10); // 10 days from now (too far)
    let end_time = start_time + Duration::hours(1); // 1 hour duration (valid)
    
    let result = quota_helper.validate_quota_limits_with_class(
        guild_id,
        user_id,
        &[],
        Some(class_id),
        start_time,
        end_time,
        None
    ).await?;
    
    assert!(!result.is_success());
    assert!(result.error_message().unwrap().contains("Maximum lead time"));
    
    // Test 4: Valid reservation (2 hours from now, 1 hour duration)
    let start_time = now + Duration::hours(2); // 2 hours from now (valid lead time)
    let end_time = start_time + Duration::hours(1); // 1 hour duration (valid)
    
    let result = quota_helper.validate_quota_limits_with_class(
        guild_id,
        user_id,
        &[],
        Some(class_id),
        start_time,
        end_time,
        None
    ).await?;
    
    assert!(result.is_success());
    
    Ok(())
}