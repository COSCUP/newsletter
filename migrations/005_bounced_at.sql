-- Add bounced_at column to track hard-bounced email addresses
ALTER TABLE subscribers ADD COLUMN IF NOT EXISTS bounced_at TIMESTAMPTZ;
