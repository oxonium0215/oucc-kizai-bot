// Utility functions
use anyhow::Result;
use serenity::model::prelude::*;
use serenity::prelude::*;

pub async fn is_admin(ctx: &Context, guild_id: GuildId, user_id: UserId) -> Result<bool> {
    let member = guild_id.member(ctx, user_id).await?;

    // Check if user has administrator permission in any context
    // Since we don't have a specific channel, we use the base guild permissions
    if let Some(guild) = guild_id.to_guild_cached(&ctx.cache) {
        let base_permissions = guild.member_permissions(&member);
        if base_permissions.administrator() {
            return Ok(true);
        }
    }

    // TODO: Check custom admin roles from database

    Ok(false)
}

/// Check if the bot has required permissions in a channel for setup
pub async fn check_bot_permissions(ctx: &Context, channel_id: ChannelId) -> Result<Vec<String>> {
    let channel = channel_id.to_channel(&ctx.http).await?;
    let current_user_id = ctx.cache.current_user().id;

    let mut missing_permissions = Vec::new();

    if let Some(guild_channel) = channel.guild() {
        let guild_id = guild_channel.guild_id;
        let member = guild_id.member(ctx, current_user_id).await?;
        let permissions = guild_channel.permissions_for_user(ctx, &member)?;

        if !permissions.contains(Permissions::SEND_MESSAGES) {
            missing_permissions.push("Send Messages".to_string());
        }
        if !permissions.contains(Permissions::VIEW_CHANNEL) {
            missing_permissions.push("Read Messages/View Channel".to_string());
        }
        if !permissions.contains(Permissions::MANAGE_MESSAGES) {
            missing_permissions.push("Manage Messages".to_string());
        }
        if !permissions.contains(Permissions::EMBED_LINKS) {
            missing_permissions.push("Embed Links".to_string());
        }
        if !permissions.contains(Permissions::READ_MESSAGE_HISTORY) {
            missing_permissions.push("Read Message History".to_string());
        }
    }

    Ok(missing_permissions)
}

pub fn format_duration_minutes(minutes: i64) -> String {
    if minutes < 60 {
        format!("{}分", minutes)
    } else {
        let hours = minutes / 60;
        let remaining_minutes = minutes % 60;
        if remaining_minutes == 0 {
            format!("{}時間", hours)
        } else {
            format!("{}時間{}分", hours, remaining_minutes)
        }
    }
}
