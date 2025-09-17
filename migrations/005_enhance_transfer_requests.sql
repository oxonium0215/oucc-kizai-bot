-- Enhance transfer_requests table for immediate and scheduled transfers
-- Adds support for both immediate handoffs and future scheduled transfers

-- Add requested_by_user_id to track who initiated the transfer (may differ from from_user_id for admin transfers)
ALTER TABLE transfer_requests ADD COLUMN requested_by_user_id INTEGER;

-- Add execute_at_utc for scheduled transfers (NULL for immediate transfers)
ALTER TABLE transfer_requests ADD COLUMN execute_at_utc DATETIME;

-- Add note field for optional transfer notes
ALTER TABLE transfer_requests ADD COLUMN note TEXT;

-- Add canceled_at_utc and canceled_by_user_id for cancellation tracking
ALTER TABLE transfer_requests ADD COLUMN canceled_at_utc DATETIME;
ALTER TABLE transfer_requests ADD COLUMN canceled_by_user_id INTEGER;

-- Update existing records to set requested_by_user_id = from_user_id for compatibility
UPDATE transfer_requests SET requested_by_user_id = from_user_id WHERE requested_by_user_id IS NULL;

-- Add indexes for scheduled transfer queries
CREATE INDEX idx_transfer_requests_execute_at ON transfer_requests (execute_at_utc, status);
CREATE INDEX idx_transfer_requests_reservation ON transfer_requests (reservation_id, status);
CREATE INDEX idx_transfer_requests_status ON transfer_requests (status, created_at);