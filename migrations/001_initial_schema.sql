-- Initial database schema for Equipment Lending Management Bot

-- Guild configuration
CREATE TABLE guilds (
    id INTEGER PRIMARY KEY,
    reservation_channel_id INTEGER,
    admin_roles TEXT, -- JSON array of role IDs
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Tags for organizing equipment
CREATE TABLE tags (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    UNIQUE (guild_id, name)
);

-- Equipment items
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

-- Lending/return locations
CREATE TABLE locations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    name TEXT NOT NULL,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    UNIQUE (guild_id, name)
);

-- Reservations
CREATE TABLE reservations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    equipment_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    start_time DATETIME NOT NULL,
    end_time DATETIME NOT NULL,
    location TEXT,
    status TEXT NOT NULL DEFAULT 'Confirmed', -- Confirmed, Cancelled
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (equipment_id) REFERENCES equipment (id) ON DELETE CASCADE
);

-- Equipment operation log
CREATE TABLE equipment_logs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    equipment_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    action TEXT NOT NULL, -- Reserved, Loaned, Returned, etc.
    location TEXT,
    previous_status TEXT,
    new_status TEXT,
    notes TEXT,
    timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (equipment_id) REFERENCES equipment (id) ON DELETE CASCADE
);

-- Transfer requests
CREATE TABLE transfer_requests (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    reservation_id INTEGER NOT NULL,
    from_user_id INTEGER NOT NULL,
    to_user_id INTEGER NOT NULL,
    expires_at DATETIME NOT NULL,
    status TEXT NOT NULL DEFAULT 'Pending', -- Pending, Accepted, Denied, Expired
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (reservation_id) REFERENCES reservations (id) ON DELETE CASCADE
);

-- Background jobs queue
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_type TEXT NOT NULL,
    payload TEXT NOT NULL, -- JSON payload
    scheduled_for DATETIME NOT NULL,
    status TEXT NOT NULL DEFAULT 'Pending', -- Pending, Running, Completed, Failed
    attempts INTEGER DEFAULT 0,
    max_attempts INTEGER DEFAULT 3,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Managed Discord messages
CREATE TABLE managed_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    guild_id INTEGER NOT NULL,
    channel_id INTEGER NOT NULL,
    message_id INTEGER NOT NULL,
    message_type TEXT NOT NULL, -- EquipmentEmbed, OverallManagement, Guide
    equipment_id INTEGER, -- NULL for non-equipment messages
    sort_order INTEGER,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (guild_id) REFERENCES guilds (id) ON DELETE CASCADE,
    FOREIGN KEY (equipment_id) REFERENCES equipment (id) ON DELETE CASCADE,
    UNIQUE (guild_id, message_id)
);

-- Create indexes for performance
CREATE INDEX idx_reservations_equipment_time ON reservations (equipment_id, start_time, end_time);
CREATE INDEX idx_reservations_user ON reservations (user_id);
CREATE INDEX idx_equipment_guild ON equipment (guild_id);
CREATE INDEX idx_jobs_scheduled ON jobs (scheduled_for, status);
CREATE INDEX idx_managed_messages_guild_channel ON managed_messages (guild_id, channel_id);