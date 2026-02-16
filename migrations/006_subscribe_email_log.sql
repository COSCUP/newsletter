CREATE TABLE IF NOT EXISTS subscribe_email_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) NOT NULL,
    ip_address INET NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_subscribe_email_log_email_created
    ON subscribe_email_log(email, created_at);
CREATE INDEX IF NOT EXISTS idx_subscribe_email_log_ip_created
    ON subscribe_email_log(ip_address, created_at);
