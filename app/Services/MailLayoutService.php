<?php

namespace App\Services;

use App\Models\AppSetting;

class MailLayoutService
{
    /** @var array<string, mixed>|null */
    private static ?array $brandingCache = null;

    /**
     * @return array{
     *     app_name: string,
     *     logo_url: ?string,
     *     frontend_url: string,
     *     year: string,
     *     primary: string,
     *     primary_dark: string,
     *     primary_light: string
     * }
     */
    public static function branding(): array
    {
        if (self::$brandingCache !== null) {
            return self::$brandingCache;
        }

        $logoSetting = AppSetting::where('key', 'logo')->first();
        $logoUrl = null;

        if ($logoSetting) {
            $media = $logoSetting->getFirstMedia('app-settings');
            if ($media) {
                $logoUrl = self::absoluteUrl($media->getUrl());
            }
        }

        $palette = BrandColorService::palette();

        self::$brandingCache = [
            'app_name' => (string) AppSetting::getValue('app_name', 'Arumanis'),
            'logo_url' => $logoUrl,
            'frontend_url' => FrontendUrlService::base(),
            'year' => (string) now()->year,
            'primary' => $palette['primary'],
            'primary_dark' => $palette['primary_dark'],
            'primary_light' => $palette['primary_light'],
        ];

        return self::$brandingCache;
    }

    public static function wrapDocument(string $innerHtml, ?string $preheader = null): string
    {
        $brand = self::branding();
        $appName = htmlspecialchars($brand['app_name'], ENT_QUOTES, 'UTF-8');
        $frontendUrl = htmlspecialchars($brand['frontend_url'], ENT_QUOTES, 'UTF-8');
        $year = htmlspecialchars($brand['year'], ENT_QUOTES, 'UTF-8');
        $preheaderText = htmlspecialchars(trim(strip_tags($preheader ?? '')), ENT_QUOTES, 'UTF-8');
        $primary = htmlspecialchars($brand['primary'], ENT_QUOTES, 'UTF-8');
        $primaryDark = htmlspecialchars($brand['primary_dark'], ENT_QUOTES, 'UTF-8');
        $primaryLight = htmlspecialchars($brand['primary_light'], ENT_QUOTES, 'UTF-8');

        $logoBlock = '';
        if ($brand['logo_url']) {
            $logoUrl = htmlspecialchars($brand['logo_url'], ENT_QUOTES, 'UTF-8');
            $logoBlock = '<img src="'.$logoUrl.'" alt="'.$appName.'" width="140" style="display:block;max-width:140px;height:auto;border:0;" />';
        } else {
            $logoBlock = '<div style="font-size:22px;font-weight:700;color:#ffffff;letter-spacing:0.3px;">'.$appName.'</div>';
        }

        return <<<HTML
<!DOCTYPE html>
<html lang="id">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{$appName}</title>
</head>
<body style="margin:0;padding:0;background-color:#f1f5f9;font-family:'Segoe UI',Arial,sans-serif;color:#0f172a;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;color:transparent;">{$preheaderText}</div>
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:#f1f5f9;padding:24px 12px;">
<tr>
<td align="center">
<table role="presentation" width="600" cellspacing="0" cellpadding="0" border="0" style="max-width:600px;width:100%;">
<tr>
<td style="background:linear-gradient(135deg, {$primary} 0%, {$primaryDark} 100%);border-radius:12px 12px 0 0;padding:24px 28px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0">
<tr>
<td align="left" style="vertical-align:middle;">{$logoBlock}</td>
<td align="right" style="vertical-align:middle;font-size:12px;color:{$primaryLight};opacity:0.95;">Notifikasi Resmi</td>
</tr>
</table>
</td>
</tr>
<tr>
<td style="background-color:#ffffff;border-left:1px solid #e2e8f0;border-right:1px solid #e2e8f0;padding:32px 28px;">
{$innerHtml}
</td>
</tr>
<tr>
<td style="background-color:#f8fafc;border:1px solid #e2e8f0;border-top:0;border-radius:0 0 12px 12px;padding:20px 28px;text-align:center;">
<p style="margin:0 0 10px;font-size:13px;line-height:1.6;color:#64748b;">
Pesan ini dikirim secara otomatis oleh sistem <strong style="color:#334155;">{$appName}</strong>.
Harap tidak membalas email ini.
</p>
<p style="margin:0 0 12px;font-size:12px;line-height:1.6;color:#94a3b8;">
<a href="{$frontendUrl}" style="color:{$primary};text-decoration:none;font-weight:600;">{$frontendUrl}</a>
</p>
<p style="margin:0;font-size:11px;line-height:1.5;color:#94a3b8;letter-spacing:0.2px;">
© {$year} {$appName}. Seluruh hak cipta dilindungi.
</p>
</td>
</tr>
</table>
</td>
</tr>
</table>
</body>
</html>
HTML;
    }

    public static function wrapPlainDocument(string $text): string
    {
        $brand = self::branding();
        $divider = str_repeat('─', 48);

        return implode("\n", array_filter([
            $brand['app_name'],
            $divider,
            trim($text),
            $divider,
            'Pesan ini dikirim secara otomatis oleh sistem '.$brand['app_name'].'. Harap tidak membalas email ini.',
            $brand['frontend_url'],
            '© '.$brand['year'].' '.$brand['app_name'].'. Seluruh hak cipta dilindungi.',
        ]));
    }

    public static function heading(string $text, ?string $subtitle = null): string
    {
        $title = htmlspecialchars($text, ENT_QUOTES, 'UTF-8');
        $subtitleHtml = $subtitle
            ? '<p style="margin:8px 0 0;font-size:15px;line-height:1.6;color:#64748b;">'.htmlspecialchars($subtitle, ENT_QUOTES, 'UTF-8').'</p>'
            : '';

        return '<h1 style="margin:0 0 20px;font-size:24px;line-height:1.3;font-weight:700;color:#0f172a;">'.$title.'</h1>'.$subtitleHtml;
    }

    public static function greeting(string $name): string
    {
        return '<p style="margin:0 0 16px;font-size:15px;line-height:1.7;color:#334155;">Halo <strong>'.htmlspecialchars($name, ENT_QUOTES, 'UTF-8').'</strong>,</p>';
    }

    public static function paragraph(string $text): string
    {
        return '<p style="margin:0 0 16px;font-size:15px;line-height:1.7;color:#334155;">'.nl2br(htmlspecialchars($text, ENT_QUOTES, 'UTF-8')).'</p>';
    }

    public static function paragraphHtml(string $html): string
    {
        return '<p style="margin:0 0 16px;font-size:15px;line-height:1.7;color:#334155;">'.$html.'</p>';
    }

    public static function button(string $label, string $url): string
    {
        $brand = self::branding();
        $primary = htmlspecialchars($brand['primary'], ENT_QUOTES, 'UTF-8');
        $safeLabel = htmlspecialchars($label, ENT_QUOTES, 'UTF-8');
        $safeUrl = str_contains($url, '{{')
            ? $url
            : htmlspecialchars($url, ENT_QUOTES, 'UTF-8');

        return <<<HTML
<table role="presentation" cellspacing="0" cellpadding="0" border="0" style="margin:24px 0 8px;">
<tr>
<td align="center" style="border-radius:8px;background-color:{$primary};">
<a href="{$safeUrl}" style="display:inline-block;padding:14px 28px;font-size:15px;font-weight:600;color:#ffffff;text-decoration:none;border-radius:8px;">{$safeLabel}</a>
</td>
</tr>
</table>
<p style="margin:8px 0 0;font-size:12px;line-height:1.6;color:#94a3b8;word-break:break-all;">Atau salin tautan: <a href="{$safeUrl}" style="color:{$primary};">{$safeUrl}</a></p>
HTML;
    }

    public static function infoBox(string $content): string
    {
        $primary = htmlspecialchars(self::branding()['primary'], ENT_QUOTES, 'UTF-8');

        return '<div style="margin:20px 0;padding:16px 18px;background-color:#f8fafc;border:1px solid #e2e8f0;border-left:4px solid '.$primary.';border-radius:8px;font-size:14px;line-height:1.7;color:#334155;">'.$content.'</div>';
    }

    public static function checkItem(string $text): string
    {
        $primary = htmlspecialchars(self::branding()['primary'], ENT_QUOTES, 'UTF-8');

        return '<div style="margin:0 0 8px;"><strong style="color:'.$primary.';">✓</strong> '.htmlspecialchars($text, ENT_QUOTES, 'UTF-8').'</div>';
    }

    public static function bulletList(array $items): string
    {
        $lis = '';
        foreach ($items as $item) {
            $lis .= '<li style="margin:0 0 8px;font-size:15px;line-height:1.6;color:#334155;">'.$item.'</li>';
        }

        return '<ul style="margin:0 0 16px;padding-left:20px;">'.$lis.'</ul>';
    }

    public static function messageBlock(string $message): string
    {
        $escaped = htmlspecialchars($message, ENT_QUOTES, 'UTF-8');

        return self::infoBox('<div style="white-space:pre-wrap;">'.$escaped.'</div>');
    }

    public static function absoluteUrl(string $url): string
    {
        if ($url === '') {
            return $url;
        }

        if (str_starts_with($url, 'http://') || str_starts_with($url, 'https://')) {
            return $url;
        }

        return rtrim((string) config('app.url'), '/').'/'.ltrim($url, '/');
    }
}