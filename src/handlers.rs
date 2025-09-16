// Handlers module - placeholder for now
use serenity::async_trait;
use serenity::model::prelude::*;
use serenity::prelude::*;
use sqlx::SqlitePool;
use tracing::{info, error};

pub struct Handler {
    db: SqlitePool,
}

impl Handler {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, ready: Ready) {
        info!("{} is connected!", ready.user.name);
    }

    async fn guild_create(&self, _ctx: Context, guild: Guild, _is_new: Option<bool>) {
        info!("Joined guild: {} ({})", guild.name, guild.id);
        
        // Initialize guild in database if not exists
        if let Err(e) = self.ensure_guild_exists(guild.id.get() as i64).await {
            error!("Failed to initialize guild {}: {}", guild.id, e);
        }
    }
}

impl Handler {
    async fn ensure_guild_exists(&self, guild_id: i64) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT OR IGNORE INTO guilds (id) VALUES (?)"
        )
        .bind(guild_id)
        .execute(&self.db)
        .await?;
        
        Ok(())
    }
}