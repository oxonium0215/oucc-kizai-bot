use anyhow::Result;
use chrono::{DateTime, Utc};
use oucc_kizai_bot::{database, traits::*, time::*};
use oucc_kizai_bot::models::{Guild as DbGuild, Tag, Location, Equipment, Reservation, Job, ManagedMessage};
use sqlx::SqlitePool;
use std::sync::Arc;
use tempfile::NamedTempFile;

/// Test helper for setting up a temporary database with migrations
pub async fn setup_test_db() -> Result<SqlitePool> {
    // Create a temporary file for the database
    let temp_file = NamedTempFile::new()?;
    let db_path = temp_file.path().to_str().unwrap();
    let database_url = format!("sqlite:{}", db_path);
    
    // Initialize database with migrations
    let pool = database::init(&database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    
    Ok(pool)
}

/// Test helper for setting up in-memory database (faster but shared)
pub async fn setup_memory_db() -> Result<SqlitePool> {
    let database_url = "sqlite::memory:";
    let pool = database::init(database_url).await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// Builder for creating test guilds
pub struct GuildBuilder {
    id: i64,
    reservation_channel_id: Option<i64>,
    admin_roles: Option<String>,
}

impl GuildBuilder {
    pub fn new(id: i64) -> Self {
        Self {
            id,
            reservation_channel_id: None,
            admin_roles: None,
        }
    }
    
    pub fn with_reservation_channel(mut self, channel_id: i64) -> Self {
        self.reservation_channel_id = Some(channel_id);
        self
    }
    
    pub fn with_admin_roles(mut self, roles: Vec<i64>) -> Self {
        self.admin_roles = Some(serde_json::to_string(&roles).unwrap());
        self
    }
    
    pub async fn build(self, db: &SqlitePool) -> Result<DbGuild> {
        let now = Utc::now();
        
        sqlx::query!(
            "INSERT INTO guilds (id, reservation_channel_id, admin_roles, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?)",
            self.id,
            self.reservation_channel_id,
            self.admin_roles,
            now,
            now
        )
        .execute(db)
        .await?;
        
        Ok(DbGuild {
            id: self.id,
            reservation_channel_id: self.reservation_channel_id,
            admin_roles: self.admin_roles,
            created_at: now,
            updated_at: now,
        })
    }
}

/// Builder for creating test tags
pub struct TagBuilder {
    guild_id: i64,
    name: String,
    sort_order: i64,
}

impl TagBuilder {
    pub fn new(guild_id: i64, name: &str) -> Self {
        Self {
            guild_id,
            name: name.to_string(),
            sort_order: 0,
        }
    }
    
    pub fn with_sort_order(mut self, order: i64) -> Self {
        self.sort_order = order;
        self
    }
    
    pub async fn build(self, db: &SqlitePool) -> Result<Tag> {
        let now = Utc::now();
        
        let result = sqlx::query!(
            "INSERT INTO tags (guild_id, name, sort_order, created_at) VALUES (?, ?, ?, ?) RETURNING id",
            self.guild_id,
            self.name,
            self.sort_order,
            now
        )
        .fetch_one(db)
        .await?;
        
        Ok(Tag {
            id: result.id,
            guild_id: self.guild_id,
            name: self.name,
            sort_order: self.sort_order,
            created_at: now,
        })
    }
}

/// Builder for creating test locations
pub struct LocationBuilder {
    guild_id: i64,
    name: String,
}

impl LocationBuilder {
    pub fn new(guild_id: i64, name: &str) -> Self {
        Self {
            guild_id,
            name: name.to_string(),
        }
    }
    
    pub async fn build(self, db: &SqlitePool) -> Result<Location> {
        let now = Utc::now();
        
        let result = sqlx::query!(
            "INSERT INTO locations (guild_id, name, created_at) VALUES (?, ?, ?) RETURNING id",
            self.guild_id,
            self.name,
            now
        )
        .fetch_one(db)
        .await?;
        
        Ok(Location {
            id: result.id,
            guild_id: self.guild_id,
            name: self.name,
            created_at: now,
        })
    }
}

/// Builder for creating test equipment
pub struct EquipmentBuilder {
    guild_id: i64,
    tag_id: Option<i64>,
    name: String,
    status: String,
    current_location: Option<String>,
    unavailable_reason: Option<String>,
    default_return_location: Option<String>,
    message_id: Option<i64>,
}

impl EquipmentBuilder {
    pub fn new(guild_id: i64, name: &str) -> Self {
        Self {
            guild_id,
            tag_id: None,
            name: name.to_string(),
            status: "Available".to_string(),
            current_location: None,
            unavailable_reason: None,
            default_return_location: None,
            message_id: None,
        }
    }
    
    pub fn with_tag(mut self, tag_id: i64) -> Self {
        self.tag_id = Some(tag_id);
        self
    }
    
    pub fn with_status(mut self, status: &str) -> Self {
        self.status = status.to_string();
        self
    }
    
    pub fn with_location(mut self, location: &str) -> Self {
        self.current_location = Some(location.to_string());
        self
    }
    
    pub fn with_default_return_location(mut self, location: &str) -> Self {
        self.default_return_location = Some(location.to_string());
        self
    }
    
    pub async fn build(self, db: &SqlitePool) -> Result<Equipment> {
        let now = Utc::now();
        
        let result = sqlx::query!(
            "INSERT INTO equipment (guild_id, tag_id, name, status, current_location, unavailable_reason, 
                                  default_return_location, message_id, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            self.guild_id,
            self.tag_id,
            self.name,
            self.status,
            self.current_location,
            self.unavailable_reason,
            self.default_return_location,
            self.message_id,
            now,
            now
        )
        .fetch_one(db)
        .await?;
        
        Ok(Equipment {
            id: result.id,
            guild_id: self.guild_id,
            tag_id: self.tag_id,
            name: self.name,
            status: self.status,
            current_location: self.current_location,
            unavailable_reason: self.unavailable_reason,
            default_return_location: self.default_return_location,
            message_id: self.message_id,
            created_at: now,
            updated_at: now,
        })
    }
}

/// Builder for creating test reservations
pub struct ReservationBuilder {
    equipment_id: i64,
    user_id: i64,
    start_time: DateTime<Utc>,
    end_time: DateTime<Utc>,
    location: Option<String>,
    status: String,
}

impl ReservationBuilder {
    pub fn new(equipment_id: i64, user_id: i64, start_time: DateTime<Utc>, end_time: DateTime<Utc>) -> Self {
        Self {
            equipment_id,
            user_id,
            start_time,
            end_time,
            location: None,
            status: "Confirmed".to_string(),
        }
    }
    
    pub fn with_location(mut self, location: &str) -> Self {
        self.location = Some(location.to_string());
        self
    }
    
    pub fn with_status(mut self, status: &str) -> Self {
        self.status = status.to_string();
        self
    }
    
    pub async fn build(self, db: &SqlitePool) -> Result<Reservation> {
        let now = Utc::now();
        
        let result = sqlx::query!(
            "INSERT INTO reservations (equipment_id, user_id, start_time, end_time, location, status, created_at, updated_at) 
             VALUES (?, ?, ?, ?, ?, ?, ?, ?) RETURNING id",
            self.equipment_id,
            self.user_id,
            self.start_time,
            self.end_time,
            self.location,
            self.status,
            now,
            now
        )
        .fetch_one(db)
        .await?;
        
        Ok(Reservation {
            id: result.id,
            equipment_id: self.equipment_id,
            user_id: self.user_id,
            start_time: self.start_time,
            end_time: self.end_time,
            location: self.location,
            status: self.status,
            created_at: now,
            updated_at: now,
            returned_at: None,
            return_location: None,
        })
    }
}

/// Test application context with mocked dependencies
pub struct TestContext {
    pub db: SqlitePool,
    pub discord_api: Arc<MockDiscordApi>,
    pub clock: Arc<TestClock>,
}

impl TestContext {
    pub async fn new() -> Result<Self> {
        let db = setup_memory_db().await?;
        let discord_api = Arc::new(MockDiscordApi::new());
        let clock = Arc::new(TestClock::new(Utc::now()));
        
        Ok(Self {
            db,
            discord_api,
            clock,
        })
    }
    
    pub async fn new_with_time(initial_time: DateTime<Utc>) -> Result<Self> {
        let db = setup_memory_db().await?;
        let discord_api = Arc::new(MockDiscordApi::new());
        let clock = Arc::new(TestClock::new(initial_time));
        
        Ok(Self {
            db,
            discord_api,
            clock,
        })
    }
    
    /// Manually trigger job processing (for testing job workers)
    pub async fn process_jobs(&self) -> Result<()> {
        // This will be implemented when we test the job worker
        // For now, just return Ok
        Ok(())
    }
}

/// Helper to create common test data
pub async fn create_test_setup(ctx: &TestContext) -> Result<(DbGuild, Tag, Location, Equipment)> {
    let guild_id = 123456789i64;
    let channel_id = 987654321i64;
    
    // Create guild
    let guild = GuildBuilder::new(guild_id)
        .with_reservation_channel(channel_id)
        .build(&ctx.db)
        .await?;
    
    // Create tag
    let tag = TagBuilder::new(guild_id, "Camera")
        .with_sort_order(1)
        .build(&ctx.db)
        .await?;
    
    // Create location
    let location = LocationBuilder::new(guild_id, "Club Room")
        .build(&ctx.db)
        .await?;
    
    // Create equipment
    let equipment = EquipmentBuilder::new(guild_id, "Sony A7")
        .with_tag(tag.id)
        .with_default_return_location("Club Room")
        .build(&ctx.db)
        .await?;
    
    Ok((guild, tag, location, equipment))
}

/// Utility to advance test clock and process jobs
pub async fn advance_time_and_process_jobs(
    ctx: &TestContext,
    duration: chrono::Duration,
) -> Result<()> {
    ctx.clock.advance(duration).await;
    ctx.process_jobs().await
}