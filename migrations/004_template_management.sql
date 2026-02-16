ALTER TABLE newsletter_templates ADD COLUMN IF NOT EXISTS description TEXT NOT NULL DEFAULT '';
ALTER TABLE newsletter_templates ADD COLUMN IF NOT EXISTS created_by VARCHAR(255);
