-- Add maintenance windows for equipment blackout periods
-- This allows admins to mark equipment as unavailable during maintenance

-- Maintenance windows table
CREATE TABLE maintenance_windows (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    equipment_id INTEGER NOT NULL,
    start_utc DATETIME NOT NULL, -- YYYY-MM-DD HH:MM:SS in UTC
    end_utc DATETIME NOT NULL,   -- YYYY-MM-DD HH:MM:SS in UTC
    reason TEXT,                 -- Optional reason for maintenance (e.g., "Cleaning", "Repair")
    created_by_user_id INTEGER NOT NULL,
    created_at_utc DATETIME DEFAULT CURRENT_TIMESTAMP,
    canceled_at_utc DATETIME,    -- NULL if not canceled
    canceled_by_user_id INTEGER, -- NULL if not canceled
    FOREIGN KEY (equipment_id) REFERENCES equipment (id) ON DELETE CASCADE,
    CHECK (start_utc < end_utc)  -- Ensure start is before end
);

-- Indexes for performance
CREATE INDEX idx_maintenance_windows_equipment_start ON maintenance_windows (equipment_id, start_utc);
CREATE INDEX idx_maintenance_windows_equipment_end ON maintenance_windows (equipment_id, end_utc);
CREATE INDEX idx_maintenance_windows_timerange ON maintenance_windows (equipment_id, start_utc, end_utc) 
    WHERE canceled_at_utc IS NULL;

-- Add settings for admin pre-start reminders (optional feature)
-- Extend existing settings if they exist, or create simple approach
-- For now, we'll add a simple table for maintenance settings
CREATE TABLE maintenance_settings (
    guild_id INTEGER PRIMARY KEY,
    admin_reminder_minutes INTEGER, -- NULL means disabled, otherwise minutes before start
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE
);