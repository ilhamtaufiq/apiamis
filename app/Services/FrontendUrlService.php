<?php

namespace App\Services;

use App\Models\AppSetting;

class FrontendUrlService
{
    public static function base(): string
    {
        $fromSetting = trim((string) AppSetting::getValue('frontend_url', ''));
        if ($fromSetting !== '') {
            return rtrim($fromSetting, '/');
        }

        $fromConfig = trim((string) config('app.frontend_url', ''));
        if ($fromConfig !== '') {
            return rtrim($fromConfig, '/');
        }

        $fromEnv = trim((string) env('FRONTEND_URL', ''));
        if ($fromEnv !== '') {
            return rtrim($fromEnv, '/');
        }

        return rtrim((string) config('app.url', 'http://localhost'), '/');
    }

    public static function to(string $path = '/'): string
    {
        $normalizedPath = '/'.ltrim($path, '/');

        return self::base().$normalizedPath;
    }

    public static function pengawasApp(string $path = '/'): string
    {
        $configured = trim((string) config('app.pengawas_app_url', env('PENGAWAS_APP_BASE_URL', '')));

        if ($configured !== '' && str_starts_with($configured, 'http')) {
            return rtrim($configured, '/').'/'.ltrim($path, '/');
        }

        $suffix = $configured !== '' ? trim($configured, '/') : 'pengawasan';

        return self::to($suffix.'/'.ltrim($path, '/'));
    }
}