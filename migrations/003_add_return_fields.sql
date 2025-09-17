-- Add return tracking fields to reservations table
-- Supports return flow and correction window functionality

-- Add returned_at timestamp (UTC datetime, nullable)
ALTER TABLE reservations ADD COLUMN returned_at DATETIME;

-- Add return location field (text, nullable)  
ALTER TABLE reservations ADD COLUMN return_location TEXT;

-- Add index for finding reservations that can be returned
CREATE INDEX idx_reservations_returnable 
ON reservations (equipment_id, status, returned_at, end_time);

-- Add index for correction window queries
CREATE INDEX idx_reservations_return_time 
ON reservations (returned_at);