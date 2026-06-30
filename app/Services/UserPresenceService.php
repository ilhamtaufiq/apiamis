<?php

namespace App\Services;

use App\Models\User;
use Carbon\Carbon;
use Illuminate\Support\Facades\Cache;

class UserPresenceService
{
    public const CACHE_KEY = 'presence:online_users';

    public const ONLINE_WINDOW_MINUTES = 5;

    public function heartbeat(User $user, string $app = 'portal'): void
    {
        $users = $this->allUsers();
        $users[$user->id] = $this->serializeUser($user, $app);
        $users = $this->pruneStale($users);

        $this->persist($users);
    }

    public function remove(User $user): void
    {
        $users = $this->allUsers();
        unset($users[$user->id]);
        $this->persist($users);
    }

    /**
     * @return list<array{
     *     id: int,
     *     name: string,
     *     email: string,
     *     avatar: ?string,
     *     gender: ?string,
     *     app: string,
     *     last_seen_at: string
     * }>
     */
    public function listOnline(): array
    {
        $users = $this->pruneStale($this->allUsers());
        $this->persist($users);

        usort($users, static function (array $a, array $b): int {
            return strcmp($b['last_seen_at'], $a['last_seen_at']);
        });

        return array_values($users);
    }

    /**
     * @return array<int, array<string, mixed>>
     */
    private function allUsers(): array
    {
        $users = Cache::get(self::CACHE_KEY, []);

        return is_array($users) ? $users : [];
    }

    /**
     * @return array{
     *     id: int,
     *     name: string,
     *     email: string,
     *     avatar: ?string,
     *     gender: ?string,
     *     app: string,
     *     last_seen_at: string
     * }
     */
    private function serializeUser(User $user, string $app = 'portal'): array
    {
        return [
            'id' => $user->id,
            'name' => $user->name,
            'email' => $user->email,
            'avatar' => $user->avatar,
            'gender' => $user->gender,
            'app' => $this->normalizeApp($app),
            'last_seen_at' => now()->toIso8601String(),
        ];
    }

    private function normalizeApp(string $app): string
    {
        return in_array($app, ['portal', 'pengawasan'], true) ? $app : 'portal';
    }

    /**
     * @param  array<int, array<string, mixed>>  $users
     * @return array<int, array<string, mixed>>
     */
    private function pruneStale(array $users): array
    {
        $cutoff = now()->subMinutes(self::ONLINE_WINDOW_MINUTES);

        return array_filter($users, static function (array $entry) use ($cutoff): bool {
            if (! isset($entry['last_seen_at']) || ! is_string($entry['last_seen_at'])) {
                return false;
            }

            return Carbon::parse($entry['last_seen_at'])->greaterThanOrEqualTo($cutoff);
        });
    }

    /**
     * @param  array<int, array<string, mixed>>  $users
     */
    private function persist(array $users): void
    {
        Cache::put(
            self::CACHE_KEY,
            $users,
            now()->addMinutes(self::ONLINE_WINDOW_MINUTES + 2),
        );
    }
}