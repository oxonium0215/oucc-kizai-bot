-- Performance optimization migration
-- Add performance metrics table and additional indexes for large equipment sets

-- Performance metrics table for monitoring
CREATE TABLE performance_metrics (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp DATETIME NOT NULL,
    messages_updated INTEGER DEFAULT 0,
    average_update_time_ms REAL DEFAULT 0.0,
    queue_size INTEGER DEFAULT 0,
    rate_limit_hits INTEGER DEFAULT 0,
    failed_updates INTEGER DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- Additional indexes for performance optimization
CREATE INDEX idx_equipment_guild_status ON equipment (guild_id, status);
CREATE INDEX idx_equipment_tag_status ON equipment (tag_id, status) WHERE tag_id IS NOT NULL;
CREATE INDEX idx_reservations_equipment_status ON reservations (equipment_id, status);
CREATE INDEX idx_reservations_time_range ON reservations (start_time, end_time, status);
CREATE INDEX idx_managed_messages_type_guild ON managed_messages (message_type, guild_id);
CREATE INDEX idx_managed_messages_equipment ON managed_messages (equipment_id) WHERE equipment_id IS NOT NULL;
CREATE INDEX idx_jobs_type_status ON jobs (job_type, status);
CREATE INDEX idx_performance_metrics_timestamp ON performance_metrics (timestamp);

-- Add sort_order to managed_messages for consistent ordering
-- This helps reduce flicker during batch updates
UPDATE managed_messages SET sort_order = id WHERE sort_order IS NULL;

-- Create composite index for optimal equipment queries with large datasets
CREATE INDEX idx_equipment_composite ON equipment (guild_id, status, tag_id, created_at);

-- Create index for frequent reservation lookups
CREATE INDEX idx_reservations_user_equipment ON reservations (user_id, equipment_id, status);