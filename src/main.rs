use anyhow::Result;
use dotenv::dotenv;
use serenity::prelude::*;
use std::env;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod commands;
mod config;
mod database;
mod handlers;
mod jobs;
mod models;
pub mod time;
pub mod utils;
pub mod traits;

use config::Config;
use handlers::Handler;
use jobs::JobWorker;

#[tokio::main]
async fn main() -> Result<()> {
    // Load environment variables from .env file if present
    dotenv().ok();

    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting OUCC Equipment Lending Bot");

    // Load configuration
    let config = Config::from_env()?;

    // Initialize database
    let db = database::init(&config.database_url).await?;

    // Run migrations
    sqlx::migrate!("./migrations").run(&db).await?;
    info!("Database migrations completed");

    // Start background job worker
    let job_worker = JobWorker::new(db.clone());
    let worker_handle = tokio::spawn(async move {
        if let Err(e) = job_worker.run().await {
            error!("Job worker error: {}", e);
        }
    });

    // Configure Discord intents
    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::GUILDS
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    // Create Discord client
    let mut client = Client::builder(&config.discord_token, intents)
        .event_handler(Handler::new(db.clone()))
        .await?;

    // Start the bot
    info!("Starting Discord client");

    // Handle shutdown gracefully
    tokio::select! {
        result = client.start() => {
            if let Err(e) = result {
                error!("Discord client error: {}", e);
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Received shutdown signal");
        }
    }

    // Stop the job worker
    worker_handle.abort();

    info!("Bot shutting down");
    Ok(())
}
