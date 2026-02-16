-- Update the default COSCUP template to include the logo image
UPDATE newsletter_templates
SET html_body = '<!DOCTYPE html>
<html lang="zh-TW">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{{ title }}</title>
</head>
<body style="margin:0;padding:0;background:#f4f4f4;font-family:-apple-system,BlinkMacSystemFont,''Segoe UI'',Roboto,''Noto Sans TC'',sans-serif;">
    <table width="100%" cellpadding="0" cellspacing="0" style="background:#f4f4f4;">
        <tr>
            <td align="center" style="padding:20px 0;">
                <table width="600" cellpadding="0" cellspacing="0" style="max-width:600px;width:100%;background:#ffffff;border-radius:8px;overflow:hidden;">
                    <!-- Header -->
                    <tr>
                        <td style="background:#3b9838;padding:24px 32px;text-align:center;">
                            <a href="https://coscup.org"><img src="{{ base_url }}/static/coscup-logo.svg" alt="COSCUP" style="height:36px;border:0;" /></a>
                        </td>
                    </tr>
                    <!-- Content -->
                    <tr>
                        <td style="padding:32px;color:#333333;font-size:16px;line-height:1.6;">
                            {{ content }}
                        </td>
                    </tr>
                    <!-- Footer -->
                    <tr>
                        <td style="padding:24px 32px;background:#f9f9f9;text-align:center;font-size:12px;color:#999999;">
                            <p style="margin:0 0 8px;">COSCUP — Conference for Open Source Coders, Users &amp; Promoters</p>
                            <p style="margin:0 0 8px;"><a href="https://coscup.org" style="color:#3b9838;">coscup.org</a></p>
                            <p style="margin:0;"><a href="{{ unsubscribe_url }}" style="color:#999999;">取消訂閱</a></p>
                        </td>
                    </tr>
                </table>
            </td>
        </tr>
    </table>
    {{ tracking_pixel }}
</body>
</html>'
WHERE slug = 'coscup-default';
