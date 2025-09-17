-- Add waitlist system for equipment reservation queue management
-- This allows users to register interest for equipment/time windows
-- and automatically receive offers when matching time becomes available

-- Waitlist entries table - stores user requests for equipment/time windows
CREATE TABLE waitlist_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    equipment_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    desired_start_utc DATETIME NOT NULL,     -- Start of desired time window (UTC)
    desired_end_utc DATETIME NOT NULL,       -- End of desired time window (UTC)
    created_at_utc DATETIME DEFAULT CURRENT_TIMESTAMP,
    canceled_at_utc DATETIME,                -- NULL if active, set when canceled
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    FOREIGN KEY (equipment_id) REFERENCES equipment (id) ON DELETE CASCADE,
    CHECK (desired_start_utc < desired_end_utc)
);

-- Waitlist offers table - stores offers made to waitlisted users
CREATE TABLE waitlist_offers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    waitlist_id INTEGER NOT NULL,            -- FK to waitlist_entries
    created_at_utc DATETIME DEFAULT CURRENT_TIMESTAMP,
    offer_expires_at_utc DATETIME NOT NULL,  -- When the offer expires
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, accepted, declined, expired
    reserved_reservation_id INTEGER,         -- Set when accepted, FK to reservations
    offered_window_start_utc DATETIME NOT NULL, -- Start of offered time window
    offered_window_end_utc DATETIME NOT NULL,   -- End of offered time window
    FOREIGN KEY (waitlist_id) REFERENCES waitlist_entries (id) ON DELETE CASCADE,
    FOREIGN KEY (reserved_reservation_id) REFERENCES reservations (id) ON DELETE SET NULL,
    CHECK (offered_window_start_utc < offered_window_end_utc),
    CHECK (status IN ('pending', 'accepted', 'declined', 'expired'))
);

-- Add waitlist settings to guilds table (offer hold duration)
ALTER TABLE guilds ADD COLUMN offer_hold_minutes INTEGER DEFAULT 15;

-- Unique constraint: prevent duplicate active entries by same user for same equipment+window
-- This ensures one user can't have multiple active waitlist entries for overlapping time windows
CREATE UNIQUE INDEX idx_waitlist_unique_active_entry 
    ON waitlist_entries (guild_id, equipment_id, user_id, desired_start_utc, desired_end_utc) 
    WHERE canceled_at_utc IS NULL;

-- Performance indexes for efficient queries
CREATE INDEX idx_waitlist_entries_equipment_time 
    ON waitlist_entries (equipment_id, desired_start_utc, desired_end_utc)
    WHERE canceled_at_utc IS NULL;

CREATE INDEX idx_waitlist_entries_user 
    ON waitlist_entries (user_id, created_at_utc) 
    WHERE canceled_at_utc IS NULL;

CREATE INDEX idx_waitlist_entries_created_order 
    ON waitlist_entries (equipment_id, created_at_utc) 
    WHERE canceled_at_utc IS NULL;

CREATE INDEX idx_waitlist_offers_waitlist 
    ON waitlist_offers (waitlist_id, status);

CREATE INDEX idx_waitlist_offers_expiry 
    ON waitlist_offers (offer_expires_at_utc, status)
    WHERE status = 'pending';

-- Index for scheduler to find expired offers
CREATE INDEX idx_waitlist_offers_pending_expired 
    ON waitlist_offers (offer_expires_at_utc)
    WHERE status = 'pending';