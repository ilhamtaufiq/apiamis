<?php

namespace App\Services\Procurement;

use App\Models\SpseSession;
use Illuminate\Support\Facades\Date;

class SpseSessionStore
{
    public function __construct(
        private readonly SpseCookieParser $cookieParser,
        private readonly SpseHttpClient $httpClient,
    ) {
    }

    public function activeSession(int $userId): ?SpseSession
    {
        return SpseSession::activeForUser($userId)->first();
    }

    public function save(int $userId, ?string $cookieHeader, ?array $cookies, ?string $lpseSlug = null): SpseSession
    {
        $parsed = $this->cookieParser->parse($cookieHeader, $cookies);

        if ($parsed === []) {
            throw new \InvalidArgumentException('Cookie SPSE tidak valid atau kosong.');
        }

        if (! collect($parsed)->contains(fn ($c) => strtoupper($c['name']) === 'SPSE_SESSION')) {
            throw new \InvalidArgumentException('Cookie SPSE_SESSION wajib ada. Pastikan sudah login ke SPSE.');
        }

        SpseSession::query()
            ->where('user_id', $userId)
            ->update(['is_active' => false]);

        $session = SpseSession::create([
            'user_id' => $userId,
            'encrypted_cookies' => $parsed,
            'lpse_slug' => $lpseSlug ?: config('services.spse.lpse_slug', 'cianjurkab'),
            'expires_at' => now()->addHours(8),
            'last_validated_at' => now(),
            'is_active' => true,
        ]);

        if (! $this->httpClient->validateSession($session)) {
            $session->update(['is_active' => false]);
            throw new \RuntimeException('Session SPSE tidak valid. Login ulang dan kirim cookie lagi.');
        }

        return $session->fresh();
    }

    public function revoke(int $userId): void
    {
        SpseSession::query()
            ->where('user_id', $userId)
            ->update(['is_active' => false]);
    }

    public function status(int $userId): array
    {
        $session = $this->activeSession($userId);

        if (! $session) {
            return [
                'connected' => false,
                'is_active' => false,
                'message' => 'Belum ada session SPSE. Login manual di SPSE lalu kirim cookie.',
            ];
        }

        $valid = $this->httpClient->validateSession($session);

        if (! $valid) {
            $session->update(['is_active' => false]);

            return [
                'connected' => false,
                'is_active' => false,
                'message' => 'Session SPSE expired. Login ulang di SPSE.',
                'expired_at' => $session->expires_at?->toIso8601String(),
            ];
        }

        $session->update(['last_validated_at' => Date::now()]);

        return [
            'connected' => true,
            'is_active' => true,
            'lpse_slug' => $session->lpse_slug,
            'last_validated_at' => $session->last_validated_at?->toIso8601String(),
            'expires_at' => $session->expires_at?->toIso8601String(),
            'message' => 'Session SPSE aktif.',
        ];
    }
}