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
