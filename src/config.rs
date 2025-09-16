use anyhow::{anyhow, Result};
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub discord_token: String,
    pub database_url: String,
    pub log_level: String,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        let discord_token = env::var("DISCORD_BOT_TOKEN")
            .map_err(|_| anyhow!("DISCORD_BOT_TOKEN environment variable is required"))?;

        let database_url = env::var("DATABASE_URL")
            .unwrap_or_else(|_| "sqlite:./bot.db".to_string());

        let log_level = env::var("LOG_LEVEL")
            .unwrap_or_else(|_| "info".to_string());

        Ok(Self {
            discord_token,
            database_url,
            log_level,
        })
    }
}