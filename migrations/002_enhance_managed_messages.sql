-- Enhanced indexes and constraints for managed messages table
-- This migration adds better indexing for the reservation channel rendering engine

-- Add enhanced index for managed messages lookup by guild, channel, and sort order
CREATE INDEX IF NOT EXISTS idx_managed_messages_guild_channel_sort 
ON managed_messages (guild_id, channel_id, sort_order);

-- Add index for message type filtering
CREATE INDEX IF NOT EXISTS idx_managed_messages_type 
ON managed_messages (guild_id, channel_id, message_type);

-- Add unique constraint to prevent duplicate header messages per channel
-- Note: SQLite doesn't support partial unique constraints, so we handle this in application logic