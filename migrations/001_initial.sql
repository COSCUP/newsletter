-- Subscribers table
CREATE TABLE IF NOT EXISTS subscribers (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) NOT NULL UNIQUE,
    name VARCHAR(255) NOT NULL DEFAULT '',
    status BOOLEAN NOT NULL DEFAULT false,
    verified_email BOOLEAN NOT NULL DEFAULT false,
    secret_code VARCHAR(64) NOT NULL,
    ucode VARCHAR(16) NOT NULL UNIQUE,
    legacy_admin_link VARCHAR(64),
    subscription_source VARCHAR(50),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Verification tokens (email verify + admin magic link)
CREATE TABLE IF NOT EXISTS verification_tokens (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    subscriber_id UUID REFERENCES subscribers(id) ON DELETE CASCADE,
    admin_email VARCHAR(255),
    token VARCHAR(64) NOT NULL UNIQUE,
    token_type VARCHAR(20) NOT NULL,
    expires_at TIMESTAMPTZ NOT NULL,
    used_at TIMESTAMPTZ
);

-- Email open/click tracking events
CREATE TABLE IF NOT EXISTS email_events (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    ucode VARCHAR(16) NOT NULL,
    event_type VARCHAR(20) NOT NULL,
    topic VARCHAR(100) NOT NULL,
    ip_address INET,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Admin sessions
CREATE TABLE IF NOT EXISTS admin_sessions (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    admin_email VARCHAR(255) NOT NULL,
    session_token VARCHAR(64) NOT NULL UNIQUE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_subscribers_email ON subscribers(email);
CREATE INDEX IF NOT EXISTS idx_subscribers_ucode ON subscribers(ucode);
CREATE INDEX IF NOT EXISTS idx_subscribers_legacy_admin_link ON subscribers(legacy_admin_link);
CREATE INDEX IF NOT EXISTS idx_verification_tokens_token ON verification_tokens(token);
CREATE INDEX IF NOT EXISTS idx_email_events_ucode ON email_events(ucode);
CREATE INDEX IF NOT EXISTS idx_email_events_topic ON email_events(topic);
CREATE INDEX IF NOT EXISTS idx_admin_sessions_session_token ON admin_sessions(session_token);
