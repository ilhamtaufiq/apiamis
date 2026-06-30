<?php

namespace App\Services;

use App\Models\AppSetting;

class MailConfigService
{
    /**
     * Apply SMTP settings from app_settings to Laravel mail config.
     * Returns true when mail is enabled and minimum config is present.
     */
    public static function applyFromSettings(?array $overrides = null): bool
    {
        $enabled = ($overrides['mail_enabled'] ?? AppSetting::getValue('mail_enabled', '0')) === '1';

        if (! $enabled) {
            return false;
        }

        $host = (string) ($overrides['mail_host'] ?? AppSetting::getValue('mail_host', 'smtp.gmail.com'));
        $port = (int) ($overrides['mail_port'] ?? AppSetting::getValue('mail_port', 587));
        $encryption = (string) ($overrides['mail_encryption'] ?? AppSetting::getValue('mail_encryption', 'tls'));
        $username = (string) ($overrides['mail_username'] ?? AppSetting::getValue('mail_username', ''));
        $password = (string) ($overrides['mail_password'] ?? AppSetting::getValue('mail_password', ''));
        $fromAddress = (string) ($overrides['mail_from_address'] ?? AppSetting::getValue('mail_from_address', $username));
        $fromName = (string) ($overrides['mail_from_name'] ?? AppSetting::getValue('mail_from_name', AppSetting::getValue('app_name', 'Arumanis')));

        if ($host === '' || $username === '' || $password === '') {
            return false;
        }

        if ($encryption === 'ssl') {
            $port = $port > 0 ? $port : 465;
            $scheme = 'smtps';
        } elseif ($encryption === 'tls') {
            $port = $port > 0 ? $port : 587;
            $scheme = null;
        } else {
            $port = $port > 0 ? $port : 587;
            $scheme = null;
        }

        config([
            'mail.default' => 'smtp',
            'mail.mailers.smtp.transport' => 'smtp',
            'mail.mailers.smtp.host' => $host,
            'mail.mailers.smtp.port' => $port,
            'mail.mailers.smtp.scheme' => $scheme,
            'mail.mailers.smtp.username' => $username,
            'mail.mailers.smtp.password' => $password,
            'mail.from.address' => $fromAddress !== '' ? $fromAddress : $username,
            'mail.from.name' => $fromName !== '' ? $fromName : 'Arumanis',
        ]);

        return true;
    }
}