-- Remove quota system and equipment classes per official documentation alignment
-- These features were not specified in the official documentation and are being removed
-- for strict adherence to documented feature set

-- Drop quota class overrides table (depends on equipment_classes)
DROP TABLE IF EXISTS quota_class_overrides;

-- Drop equipment classes table
DROP TABLE IF EXISTS equipment_classes;

-- Remove class_id column from equipment table
-- Use a safe approach that handles existing data
CREATE TABLE equipment_temp AS 
SELECT id, guild_id, tag_id, name, status, current_location, 
       unavailable_reason, default_return_location, message_id, 
       created_at, updated_at
FROM equipment;

-- Drop old equipment table
DROP TABLE equipment;

-- Recreate equipment table without class_id column
CREATE TABLE equipment (
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

-- Copy data back to new equipment table
INSERT INTO equipment (id, guild_id, tag_id, name, status, current_location, 
                      unavailable_reason, default_return_location, message_id, 
                      created_at, updated_at)
SELECT id, guild_id, tag_id, name, status, current_location, 
       unavailable_reason, default_return_location, message_id, 
       created_at, updated_at
FROM equipment_temp;

-- Drop temporary table
DROP TABLE equipment_temp;

-- Drop quota-related tables
DROP TABLE IF EXISTS quota_override_audits;
DROP TABLE IF EXISTS quota_role_overrides;  
DROP TABLE IF EXISTS quota_settings;

-- Recreate essential indexes that may have been lost
CREATE INDEX IF NOT EXISTS idx_equipment_guild ON equipment (guild_id);
CREATE INDEX IF NOT EXISTS idx_reservations_equipment_time ON reservations (equipment_id, start_time, end_time);
CREATE INDEX IF NOT EXISTS idx_reservations_user ON reservations (user_id);