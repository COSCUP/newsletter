CREATE TABLE IF NOT EXISTS admin_login_log (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    email VARCHAR(255) NOT NULL,
    ip_address INET NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_admin_login_log_email_created
    ON admin_login_log(email, created_at);
CREATE INDEX IF NOT EXISTS idx_admin_login_log_ip_created
    ON admin_login_log(ip_address, created_at);
