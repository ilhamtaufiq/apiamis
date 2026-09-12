<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Support\Facades\DB;

class ChatUserMemory extends Model
{
    protected $fillable = ['user_id', 'fact_hash', 'fact', 'score', 'hit_count'];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    public static function hashFact(string $fact): string
    {
        // Normalisasi ringan: lowercase, buang tanda baca, urutkan kata
        // — "sering tanya paket X" ≡ "paket X sering tanya".
        $words = preg_split('/\s+/u', preg_replace('/[^\w\s]/u', '', mb_strtolower(trim($fact))), -1, PREG_SPLIT_NO_EMPTY);
        sort($words);

        return hash('sha256', implode(' ', $words));
    }

    public static function learn(string $fact, int $userId): void
    {
        $fact = trim($fact);
        if (mb_strlen($fact) < 10 || mb_strlen($fact) > 300) {
            return;
        }

        self::updateOrCreate(
            ['user_id' => $userId, 'fact_hash' => self::hashFact($fact)],
            ['fact' => $fact, 'score' => DB::raw('score + 1'), 'hit_count' => DB::raw('hit_count + 1')],
        );
    }

    /**
     * Fakta tahan-lama top-K untuk system prompt.
     */
    public static function forUser(int $userId, int $limit = 8): string
    {
        $facts = self::where('user_id', $userId)
            ->where('score', '>=', 1.0)
            ->orderByDesc('score')
            ->orderByDesc('updated_at')
            ->limit($limit)
            ->pluck('fact');

        return $facts->isEmpty() ? '' : '- ' . $facts->implode("\n- ");
    }
}
