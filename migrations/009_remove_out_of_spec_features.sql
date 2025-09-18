-- Remove out-of-spec features to align with specification.md v2.3
-- This migration removes: maintenance windows, quota system, equipment classes, and waitlist features

-- Drop maintenance-related tables
DROP TABLE IF EXISTS maintenance_windows;
DROP TABLE IF EXISTS maintenance_settings;

-- Drop quota system tables
DROP TABLE IF EXISTS quota_override_audits;
DROP TABLE IF EXISTS quota_class_overrides;
DROP TABLE IF EXISTS quota_role_overrides;
DROP TABLE IF EXISTS quota_settings;

-- Drop equipment classes table
DROP TABLE IF EXISTS equipment_classes;

-- Remove class_id column from equipment table if it exists
-- Using a more compatible approach for SQLite
CREATE TABLE equipment_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    tag_id INTEGER,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Available', -- Available, Loaned, Unavailable
    current_location TEXT,
    unavailable_reason TEXT,
    default_return_location TEXT,
    message_id INTEGER, -- Discord message ID for this equipment's embed
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    FOREIGN KEY (tag_id) REFERENCES tags (id) ON DELETE SET NULL,
    UNIQUE (guild_id, name)
);

-- Copy data from old table to new table (excluding class_id)
INSERT INTO equipment_new (id, guild_id, tag_id, name, status, current_location, 
                          unavailable_reason, default_return_location, message_id, 
                          created_at, updated_at)
SELECT id, guild_id, tag_id, name, status, current_location, 
       unavailable_reason, default_return_location, message_id, 
       created_at, updated_at
FROM equipment;

-- Drop old table and rename new one
DROP TABLE equipment;
ALTER TABLE equipment_new RENAME TO equipment;

-- Recreate indexes that might have been lost
CREATE INDEX IF NOT EXISTS idx_equipment_guild_tag ON equipment (guild_id, tag_id);
CREATE INDEX IF NOT EXISTS idx_equipment_message ON equipment (message_id);

-- Drop any waitlist-related tables if they exist (future-proofing)
DROP TABLE IF EXISTS waitlist_entries;
DROP TABLE IF EXISTS waitlist_offers;

-- Remove any indexes related to dropped tables
DROP INDEX IF EXISTS idx_maintenance_windows_equipment_start;
DROP INDEX IF EXISTS idx_maintenance_windows_equipment_end;
DROP INDEX IF EXISTS idx_maintenance_windows_timerange;
DROP INDEX IF EXISTS idx_quota_settings_guild;
DROP INDEX IF EXISTS idx_quota_role_overrides_guild_role;
DROP INDEX IF EXISTS idx_quota_override_audits_guild;
DROP INDEX IF EXISTS idx_quota_override_audits_user;
DROP INDEX IF EXISTS idx_quota_override_audits_created;
DROP INDEX IF EXISTS idx_reservations_user_time_status;
DROP INDEX IF EXISTS idx_reservations_guild_user_time;
DROP INDEX IF EXISTS idx_equipment_classes_guild;
DROP INDEX IF EXISTS idx_equipment_class_id;
DROP INDEX IF EXISTS idx_quota_class_overrides_guild_class;
DROP INDEX IF EXISTS idx_equipment_guild_class;