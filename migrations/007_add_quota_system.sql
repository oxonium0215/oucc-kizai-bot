-- Add quota system for per-guild and per-role reservation limits
-- This supports configurable quotas with admin overrides and audit logging

-- Quota settings per guild
CREATE TABLE quota_settings (
    guild_id INTEGER PRIMARY KEY,
    max_active_count INTEGER,         -- NULL means unlimited
    max_overlap_count INTEGER,        -- NULL means unlimited  
    max_hours_7d INTEGER,            -- NULL means unlimited
    max_hours_30d INTEGER,           -- NULL means unlimited
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE
);

-- Role-based quota overrides (more permissive values take precedence)
CREATE TABLE quota_role_overrides (
    guild_id INTEGER NOT NULL,
    role_id INTEGER NOT NULL,
    max_active_count INTEGER,         -- NULL means use guild default
    max_overlap_count INTEGER,        -- NULL means use guild default
    max_hours_7d INTEGER,            -- NULL means use guild default
    max_hours_30d INTEGER,           -- NULL means use guild default
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (guild_id, role_id),
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE
);

-- Audit log for admin quota overrides
CREATE TABLE quota_override_audits (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    reservation_id INTEGER,           -- NULL for bulk operations or general overrides
    user_id INTEGER NOT NULL,         -- User whose quota was overridden
    acted_by_user_id INTEGER NOT NULL, -- Admin who performed the override
    reason TEXT,                      -- Optional reason for override
    created_at_utc DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    FOREIGN KEY (reservation_id) REFERENCES reservations (id) ON DELETE SET NULL
);

-- Indexes for efficient quota queries
CREATE INDEX idx_quota_settings_guild ON quota_settings (guild_id);
CREATE INDEX idx_quota_role_overrides_guild_role ON quota_role_overrides (guild_id, role_id);
CREATE INDEX idx_quota_override_audits_guild ON quota_override_audits (guild_id);
CREATE INDEX idx_quota_override_audits_user ON quota_override_audits (user_id);
CREATE INDEX idx_quota_override_audits_created ON quota_override_audits (created_at_utc);

-- Index for efficient quota calculations on reservations
-- This supports queries for active reservations and time-based aggregations
CREATE INDEX idx_reservations_user_time_status ON reservations (user_id, start_time, end_time, status, returned_at);
CREATE INDEX idx_reservations_guild_user_time ON reservations (equipment_id, user_id, start_time, end_time) 
    WHERE status = 'Confirmed' AND returned_at IS NULL;