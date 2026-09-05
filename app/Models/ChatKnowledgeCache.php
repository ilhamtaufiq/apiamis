<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class ChatKnowledgeCache extends Model
{
    protected $table = 'chat_knowledge_cache';
    
    protected $fillable = ['query_hash', 'query', 'context_summary', 'response', 'hit_count', 'quality_score'];

    /**
     * Find cached response for a similar query
     */
    public static function findSimilar(string $query): ?self
    {
        $hash = self::hashQuery($query);
        $cached = self::where('query_hash', $hash)
            ->where('quality_score', '>=', 0.5) // Only use high-quality cached responses
            ->first();
        
        if ($cached) {
            $cached->increment('hit_count');
        }
        
        return $cached;
    }

    /**
     * Store or update a learned Q&A pair
     */
    public static function learn(string $query, string $contextSummary, string $response, int $tokensUsed = 0): self
    {
        $hash = self::hashQuery($query);

        return self::updateOrCreate(
            ['query_hash' => $hash],
            [
                'query' => $query,
                'context_summary' => mb_substr($contextSummary, 0, 2000), // Limit context size
                'response' => $response,
                'quality_score' => 0.7, // Default new entries
            ]
        );
    }

    /**
     * Turunkan skor entri (vote down). Di bawah 0.5 tak dipakai cache/few-shot.
     */
    public static function downvote(string $query): ?self
    {
        $cached = self::where('query_hash', self::hashQuery($query))->first();
        if (!$cached) {
            return null;
        }

        $cached->decrement('quality_score', 0.3);

        return $cached->refresh();
    }

    /**
     * Normalize and hash the query for deduplication
     */
    public static function hashQuery(string $query): string
    {
        // 1. Basic normalization
        $normalized = strtolower(trim($query));
        
        // 2. Remove Indonesian filler/stop words to increase hit rate
        $stopWords = ['apa', 'bagaimana', 'siapa', 'dimana', 'kapan', 'tampilkan', 'lihat', 'cari', 'dong', 'sih', 'ya', 'kah', 'tolong', 'bisa', 'boleh', 'yang', 'di', 'ke', 'dari', 'dan', 'atau'];
        $words = explode(' ', preg_replace('/[^\w\s]/u', '', $normalized));
        $filteredWords = array_filter($words, fn($w) => !in_array($w, $stopWords) && strlen($w) > 1);
        
        // 3. Sort words to handle different word orders (e.g., "foto mck" vs "mck foto")
        sort($filteredWords);
        $finalQuery = implode(' ', $filteredWords);
        
        // If empty after filtering, fallback to original normalized
        if (empty($finalQuery)) {
            $finalQuery = preg_replace('/\s+/', ' ', preg_replace('/[^\w\s]/u', '', $normalized));
        }
        
        return hash('sha256', $finalQuery);
    }

    /**
     * Get top cached Q&A pairs as few-shot examples for the AI
     */
    public static function getFewShotExamples(int $limit = 3): array
    {
        return self::where('quality_score', '>=', 0.7)
            ->orderByDesc('hit_count')
            ->limit($limit)
            ->get()
            ->map(fn($c) => [
                'query' => $c->query,
                'response' => mb_substr($c->response, 0, 500), // Truncate for token savings
            ])
            ->toArray();
    }
}
