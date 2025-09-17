-- Add notification system tables and preferences
-- Supports reminder notifications with DM fallback and configurability

-- Add notification preferences to guilds table
ALTER TABLE guilds ADD COLUMN dm_fallback_channel_enabled BOOLEAN DEFAULT TRUE;
ALTER TABLE guilds ADD COLUMN overdue_repeat_hours INTEGER DEFAULT 12;
ALTER TABLE guilds ADD COLUMN overdue_max_count INTEGER DEFAULT 3;
ALTER TABLE guilds ADD COLUMN pre_start_minutes INTEGER DEFAULT 15;
ALTER TABLE guilds ADD COLUMN pre_end_minutes INTEGER DEFAULT 15;

-- Sent reminders tracking table to prevent duplicates
CREATE TABLE sent_reminders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    reservation_id INTEGER NOT NULL,
    kind TEXT NOT NULL, -- PRE_START, START, PRE_END, OVERDUE_1, OVERDUE_2, etc.
    sent_at_utc DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    delivery_method TEXT NOT NULL, -- DM, CHANNEL, FAILED
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (reservation_id) REFERENCES reservations (id) ON DELETE CASCADE,
    UNIQUE (reservation_id, kind)
);

-- Add indexes for reminder queries
CREATE INDEX idx_sent_reminders_reservation ON sent_reminders (reservation_id);
CREATE INDEX idx_sent_reminders_kind ON sent_reminders (kind, sent_at_utc);

-- Add index for notification preferences on guilds
CREATE INDEX idx_guilds_notification_prefs ON guilds (dm_fallback_channel_enabled);