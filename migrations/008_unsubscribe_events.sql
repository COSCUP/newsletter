CREATE TABLE IF NOT EXISTS unsubscribe_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscriber_id UUID NOT NULL REFERENCES subscribers(id),
    newsletter_id UUID REFERENCES newsletters(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_unsubscribe_events_newsletter_id
    ON unsubscribe_events(newsletter_id);
