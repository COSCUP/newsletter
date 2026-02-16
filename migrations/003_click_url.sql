-- Add clicked_url column to email_events for per-URL click tracking
ALTER TABLE email_events ADD COLUMN IF NOT EXISTS clicked_url TEXT;
