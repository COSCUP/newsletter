-- Update default COSCUP template logo from SVG to PNG for email client compatibility
UPDATE newsletter_templates
SET html_body = REPLACE(html_body, '/static/coscup-logo.svg', '/static/coscup-logo.png')
WHERE slug = 'coscup-default';
