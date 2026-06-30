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
     *     primary_light: string,
     *     primary_fixed: string,
     *     primary_container: string,
     *     secondary: string,
     *     secondary_fixed: string,
     *     on_secondary: string,
     *     on_secondary_fixed: string,
     *     surface: string,
     *     surface_container: string,
     *     surface_container_low: string,
     *     outline_variant: string,
     *     on_surface: string,
     *     on_surface_variant: string
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
            ...$palette,
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
        $primaryFixed = htmlspecialchars($brand['primary_fixed'], ENT_QUOTES, 'UTF-8');
        $primaryContainer = htmlspecialchars($brand['primary_container'], ENT_QUOTES, 'UTF-8');
        $secondary = htmlspecialchars($brand['secondary'], ENT_QUOTES, 'UTF-8');
        $surface = htmlspecialchars($brand['surface'], ENT_QUOTES, 'UTF-8');
        $surfaceContainerLow = htmlspecialchars($brand['surface_container_low'], ENT_QUOTES, 'UTF-8');
        $outlineVariant = htmlspecialchars($brand['outline_variant'], ENT_QUOTES, 'UTF-8');
        $onSurface = htmlspecialchars($brand['on_surface'], ENT_QUOTES, 'UTF-8');
        $onSurfaceVariant = htmlspecialchars($brand['on_surface_variant'], ENT_QUOTES, 'UTF-8');

        $logoBlock = '';
        if ($brand['logo_url']) {
            $logoUrl = htmlspecialchars($brand['logo_url'], ENT_QUOTES, 'UTF-8');
            $logoBlock = '<img src="'.$logoUrl.'" alt="'.$appName.'" width="160" style="display:block;margin:0 auto;max-width:160px;height:auto;border:0;" />';
        } else {
            $logoBlock = '<div style="font-size:28px;font-weight:800;color:'.$primary.';letter-spacing:-0.02em;text-align:center;">'.$appName.'</div>';
        }

        return <<<HTML
<!DOCTYPE html>
<html lang="id">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{$appName}</title>
<!--[if mso]>
<style type="text/css">
body, table, td {font-family: Arial, sans-serif !important;}
</style>
<![endif]-->
</head>
<body style="margin:0;padding:0;background-color:{$surface};font-family:'Segoe UI',Arial,'Plus Jakarta Sans',sans-serif;color:#1f1926;-webkit-font-smoothing:antialiased;">
<div style="display:none;max-height:0;overflow:hidden;opacity:0;color:transparent;">{$preheaderText}</div>
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:{$surface};padding:24px 12px;">
<tr>
<td align="center">
<table role="presentation" width="600" cellspacing="0" cellpadding="0" border="0" style="max-width:600px;width:100%;background-color:{$surface};border-radius:16px;overflow:hidden;box-shadow:0 4px 24px -8px {$primaryContainer};">
<tr>
<td style="background-color:{$primaryFixed};padding:36px 28px;text-align:center;position:relative;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0">
<tr>
<td align="center" style="padding:0 0 4px;">
<span style="display:inline-block;font-size:20px;line-height:1;color:{$primary};opacity:0.35;">&#10022;</span>
</td>
</tr>
<tr>
<td align="center">{$logoBlock}</td>
</tr>
<tr>
<td align="center" style="padding:8px 0 0;">
<span style="display:inline-block;font-size:11px;font-weight:700;letter-spacing:0.12em;text-transform:uppercase;color:{$onSurfaceVariant};opacity:0.85;">Notifikasi Resmi</span>
</td>
</tr>
</table>
</td>
</tr>
<tr>
<td style="background-color:{$surface};padding:28px 24px 24px;">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:#ffffff;border:1px solid {$outlineVariant};border-radius:16px;">
<tr>
<td style="padding:32px 28px;">
{$innerHtml}
</td>
</tr>
</table>
</td>
</tr>
<tr>
<td style="background-color:{$surfaceContainerLow};padding:24px 28px;text-align:center;border-top:1px solid {$outlineVariant};">
<p style="margin:0 0 6px;font-size:16px;line-height:1.4;font-weight:700;color:{$primaryDark};">{$appName}</p>
<p style="margin:0 0 14px;font-size:13px;line-height:1.6;color:{$onSurface};">
Pesan ini dikirim secara otomatis oleh sistem <strong style="color:{$primaryDark};">{$appName}</strong>.
Harap tidak membalas email ini.
</p>
<p style="margin:0 0 14px;font-size:12px;line-height:1.6;">
<a href="{$frontendUrl}" style="color:{$secondary};text-decoration:none;font-weight:600;">{$frontendUrl}</a>
</p>
<p style="margin:0;font-size:11px;line-height:1.5;color:{$onSurfaceVariant};opacity:0.85;letter-spacing:0.2px;">
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

    public static function badge(string $text): string
    {
        $brand = self::branding();
        $bg = htmlspecialchars($brand['secondary_fixed'], ENT_QUOTES, 'UTF-8');
        $color = htmlspecialchars($brand['on_secondary_fixed'], ENT_QUOTES, 'UTF-8');
        $label = htmlspecialchars(strtoupper($text), ENT_QUOTES, 'UTF-8');

        return '<p style="margin:0 0 16px;text-align:center;"><span style="display:inline-block;padding:6px 16px;background-color:'.$bg.';color:'.$color.';font-size:11px;font-weight:700;letter-spacing:0.1em;border-radius:9999px;">'.$label.'</span></p>';
    }

    public static function heading(string $text, ?string $subtitle = null, bool $centered = false): string
    {
        $brand = self::branding();
        $primaryDark = htmlspecialchars($brand['primary_dark'], ENT_QUOTES, 'UTF-8');
        $title = htmlspecialchars($text, ENT_QUOTES, 'UTF-8');
        $align = $centered ? 'center' : 'left';
        $subtitleHtml = $subtitle
            ? '<p style="margin:8px 0 0;font-size:15px;line-height:1.6;color:'.$brand['on_surface_variant'].';text-align:'.$align.';">'.htmlspecialchars($subtitle, ENT_QUOTES, 'UTF-8').'</p>'
            : '';

        return '<h1 style="margin:0 0 20px;font-size:28px;line-height:1.25;font-weight:800;color:'.$primaryDark.';letter-spacing:-0.02em;text-align:'.$align.';">'.$title.'</h1>'.$subtitleHtml;
    }

    public static function greeting(string $name): string
    {
        $color = htmlspecialchars(self::branding()['on_surface'], ENT_QUOTES, 'UTF-8');

        return '<p style="margin:0 0 16px;font-size:16px;line-height:1.7;color:'.$color.';">Halo <strong>'.htmlspecialchars($name, ENT_QUOTES, 'UTF-8').'</strong>,</p>';
    }

    public static function paragraph(string $text): string
    {
        $brand = self::branding();
        $color = htmlspecialchars($brand['on_surface_variant'], ENT_QUOTES, 'UTF-8');

        return '<p style="margin:0 0 16px;font-size:16px;line-height:1.75;color:'.$color.';">'.nl2br(htmlspecialchars($text, ENT_QUOTES, 'UTF-8')).'</p>';
    }

    public static function paragraphHtml(string $html): string
    {
        $brand = self::branding();
        $color = htmlspecialchars($brand['on_surface_variant'], ENT_QUOTES, 'UTF-8');

        return '<p style="margin:0 0 16px;font-size:16px;line-height:1.75;color:'.$color.';">'.$html.'</p>';
    }

    public static function button(string $label, string $url): string
    {
        $brand = self::branding();
        $secondary = htmlspecialchars($brand['secondary'], ENT_QUOTES, 'UTF-8');
        $onSecondary = htmlspecialchars($brand['on_secondary'], ENT_QUOTES, 'UTF-8');
        $primary = htmlspecialchars($brand['primary'], ENT_QUOTES, 'UTF-8');
        $safeLabel = htmlspecialchars($label, ENT_QUOTES, 'UTF-8');
        $safeUrl = str_contains($url, '{{')
            ? $url
            : htmlspecialchars($url, ENT_QUOTES, 'UTF-8');

        return <<<HTML
<table role="presentation" cellspacing="0" cellpadding="0" border="0" width="100%" style="margin:28px 0 8px;">
<tr>
<td align="center">
<table role="presentation" cellspacing="0" cellpadding="0" border="0">
<tr>
<td align="center" style="border-radius:9999px;background-color:{$secondary};">
<a href="{$safeUrl}" style="display:inline-block;padding:14px 32px;font-size:16px;font-weight:700;color:{$onSecondary};text-decoration:none;border-radius:9999px;">{$safeLabel}</a>
</td>
</tr>
</table>
</td>
</tr>
</table>
<p style="margin:8px 0 0;font-size:12px;line-height:1.6;color:#94a3b8;text-align:center;word-break:break-all;">Atau salin tautan: <a href="{$safeUrl}" style="color:{$primary};">{$safeUrl}</a></p>
HTML;
    }

    public static function infoBox(string $content): string
    {
        $brand = self::branding();
        $primary = htmlspecialchars($brand['primary'], ENT_QUOTES, 'UTF-8');
        $bg = htmlspecialchars($brand['surface_container_low'], ENT_QUOTES, 'UTF-8');
        $border = htmlspecialchars($brand['outline_variant'], ENT_QUOTES, 'UTF-8');
        $color = htmlspecialchars($brand['on_surface'], ENT_QUOTES, 'UTF-8');

        return '<div style="margin:20px 0;padding:16px 18px;background-color:'.$bg.';border:1px solid '.$border.';border-left:4px solid '.$primary.';border-radius:12px;font-size:14px;line-height:1.7;color:'.$color.';">'.$content.'</div>';
    }

    public static function highlightStrip(string $text): string
    {
        $brand = self::branding();
        $primary = htmlspecialchars($brand['primary'], ENT_QUOTES, 'UTF-8');
        $border = htmlspecialchars($brand['outline_variant'], ENT_QUOTES, 'UTF-8');
        $safeText = htmlspecialchars($text, ENT_QUOTES, 'UTF-8');

        return '<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="margin:20px 0;border-top:1px solid '.$border.';border-bottom:1px solid '.$border.';"><tr><td style="padding:16px 0;font-size:18px;line-height:1.5;font-weight:600;font-style:italic;color:'.$primary.';text-align:center;">&#127881; '.$safeText.'</td></tr></table>';
    }

    public static function checkItem(string $text): string
    {
        $primary = htmlspecialchars(self::branding()['primary'], ENT_QUOTES, 'UTF-8');

        return '<div style="margin:0 0 8px;"><strong style="color:'.$primary.';">✓</strong> '.htmlspecialchars($text, ENT_QUOTES, 'UTF-8').'</div>';
    }

    public static function bulletList(array $items): string
    {
        $color = htmlspecialchars(self::branding()['on_surface_variant'], ENT_QUOTES, 'UTF-8');
        $lis = '';
        foreach ($items as $item) {
            $lis .= '<li style="margin:0 0 8px;font-size:16px;line-height:1.6;color:'.$color.';">'.$item.'</li>';
        }

        return '<ul style="margin:0 0 16px;padding-left:20px;">'.$lis.'</ul>';
    }

    public static function messageBlock(string $message): string
    {
        $escaped = htmlspecialchars($message, ENT_QUOTES, 'UTF-8');

        return self::infoBox('<div style="white-space:pre-wrap;">'.$escaped.'</div>');
    }

    /**
     * @param  array<int, array{icon: string, title: string, description: string}>  $tiles
     */
    public static function infoTiles(array $tiles): string
    {
        if ($tiles === []) {
            return '';
        }

        $brand = self::branding();
        $bg = htmlspecialchars($brand['surface_container_low'], ENT_QUOTES, 'UTF-8');
        $border = htmlspecialchars($brand['outline_variant'], ENT_QUOTES, 'UTF-8');
        $primary = htmlspecialchars($brand['primary'], ENT_QUOTES, 'UTF-8');
        $descColor = htmlspecialchars($brand['on_surface_variant'], ENT_QUOTES, 'UTF-8');

        $cells = '';
        foreach ($tiles as $index => $tile) {
            $icon = htmlspecialchars($tile['icon'], ENT_QUOTES, 'UTF-8');
            $title = htmlspecialchars($tile['title'], ENT_QUOTES, 'UTF-8');
            $description = htmlspecialchars($tile['description'], ENT_QUOTES, 'UTF-8');
            $width = count($tiles) > 1 ? '50%' : '100%';
            $paddingRight = $index === 0 && count($tiles) > 1 ? ' padding-right:8px;' : '';
            $paddingLeft = $index > 0 ? ' padding-left:8px;' : '';

            $cells .= <<<HTML
<td width="{$width}" valign="top" style="width:{$width};{$paddingRight}{$paddingLeft}">
<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="background-color:{$bg};border:1px solid {$border};border-radius:12px;">
<tr>
<td style="padding:16px;">
<div style="font-size:20px;line-height:1;margin:0 0 8px;">{$icon}</div>
<div style="font-size:14px;font-weight:600;color:{$primary};margin:0 0 4px;">{$title}</div>
<div style="font-size:12px;line-height:1.5;color:{$descColor};">{$description}</div>
</td>
</tr>
</table>
</td>
HTML;
        }

        return '<table role="presentation" width="100%" cellspacing="0" cellpadding="0" border="0" style="margin:24px 0 0;"><tr>'.$cells.'</tr></table>';
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