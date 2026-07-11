<?php

namespace App\Services;

use App\Models\AppSetting;
use App\Models\User;
use Illuminate\Http\Request;

class MaintenanceModeService
{
    public const SETTING_KEY = 'maintenance_mode';

    public const BYPASS_EMAILS_SETTING_KEY = 'maintenance_bypass_emails';

    /** Default bypass when no setting/env is configured. */
    private const DEFAULT_BYPASS_EMAILS = [
        'ilhamtaufiq@gmail.com',
    ];

    public function isEnabled(): bool
    {
        $value = AppSetting::getValue(self::SETTING_KEY, '0');

        return $value === '1' || $value === 'true' || $value === 'on';
    }

    /**
     * @return list<string>
     */
    public function bypassEmails(): array
    {
        $fromSetting = trim((string) AppSetting::getValue(self::BYPASS_EMAILS_SETTING_KEY, ''));
        $fromEnv = trim((string) env('MAINTENANCE_BYPASS_EMAILS', ''));

        $raw = $fromSetting !== '' ? $fromSetting : $fromEnv;

        if ($raw === '') {
            return self::DEFAULT_BYPASS_EMAILS;
        }

        $emails = array_values(array_filter(array_map(
            static fn (string $email) => strtolower(trim($email)),
            preg_split('/[,;\s]+/', $raw) ?: []
        )));

        return $emails !== [] ? $emails : self::DEFAULT_BYPASS_EMAILS;
    }

    public function allowsEmail(?string $email): bool
    {
        if ($email === null || trim($email) === '') {
            return false;
        }

        return in_array(strtolower(trim($email)), $this->bypassEmails(), true);
    }

    public function allowsUser(?User $user): bool
    {
        return $user !== null && $this->allowsEmail($user->email);
    }

    /**
     * Paths that stay reachable during maintenance (status check, login flow).
     */
    public function isExemptPath(string $path): bool
    {
        $path = ltrim($path, '/');
        // API routes are registered without the "api/" prefix in $request->path()
        // when using RouteServiceProvider prefix — path is e.g. "api/auth/login" or "auth/login".
        $normalized = preg_replace('#^api/#', '', $path) ?? $path;

        $exact = [
            'app-settings/maintenance',
            'auth/login',
            'auth/logout',
            'auth/me',
            'auth/google',
            'auth/google/callback',
            'auth/handoff',
            'auth/handoff/exchange',
            'up',
        ];

        if (in_array($normalized, $exact, true)) {
            return true;
        }

        // Public health / static checks
        if (str_starts_with($normalized, 'public/')) {
            return false; // block public content during maintenance
        }

        return false;
    }

    public function resolveUser(Request $request): ?User
    {
        $user = $request->user() ?? $request->user('sanctum');
        if ($user instanceof User) {
            return $user;
        }

        return null;
    }

    public function statusPayload(?User $user = null): array
    {
        $enabled = $this->isEnabled();
        $bypass = $this->allowsUser($user);

        return [
            'enabled' => $enabled,
            'bypass' => $enabled && $bypass,
            'message' => $enabled
                ? 'Aplikasi sedang maintenance. Hanya akun bypass yang dapat mengakses.'
                : null,
            // Do not expose full bypass list publicly
            'can_access' => ! $enabled || $bypass,
        ];
    }
}
