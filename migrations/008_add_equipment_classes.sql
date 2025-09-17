-- Add equipment classes and class-specific quota system
-- This allows organizing equipment into classes with specific reservation limits

-- Equipment classes table
CREATE TABLE equipment_classes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    emoji TEXT,                          -- Optional emoji/icon for display
    description TEXT,                    -- Optional description
    created_at_utc DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    UNIQUE (guild_id, name)
);

-- Add class_id to equipment table (nullable - equipment without class inherits global limits only)
ALTER TABLE equipment ADD COLUMN class_id INTEGER REFERENCES equipment_classes (id) ON DELETE SET NULL;

-- Class-specific quota overrides
CREATE TABLE quota_class_overrides (
    guild_id INTEGER NOT NULL,
    class_id INTEGER NOT NULL,
    max_active_count INTEGER,            -- NULL means use guild/role effective limit
    max_overlap_count INTEGER,           -- NULL means use guild/role effective limit
    max_hours_7d INTEGER,               -- NULL means use guild/role effective limit
    max_hours_30d INTEGER,              -- NULL means use guild/role effective limit
    max_duration_hours INTEGER,         -- NULL means unlimited duration
    min_lead_time_minutes INTEGER,      -- NULL means no minimum lead time
    max_lead_time_days INTEGER,         -- NULL means unlimited lead time
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (guild_id, class_id),
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    FOREIGN KEY (class_id) REFERENCES equipment_classes (id) ON DELETE CASCADE
);

-- Indexes for efficient queries
CREATE INDEX idx_equipment_classes_guild ON equipment_classes (guild_id);
CREATE INDEX idx_equipment_class_id ON equipment (class_id);
CREATE INDEX idx_quota_class_overrides_guild_class ON quota_class_overrides (guild_id, class_id);

-- Index for equipment queries by guild and class
CREATE INDEX idx_equipment_guild_class ON equipment (guild_id, class_id);