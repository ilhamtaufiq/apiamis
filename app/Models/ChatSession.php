<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class ChatSession extends Model
{
    protected $fillable = ['user_id', 'title'];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    public function messages(): HasMany
    {
        return $this->hasMany(ChatMessage::class)->orderBy('created_at', 'asc');
    }

    /**
     * Auto-generate title from first user message (B7: pass content to skip DB query)
     */
    public function generateTitle(?string $content = null): void
    {
        if ($content === null) {
            $content = $this->messages()->where('role', 'user')->value('content');
        }
        if ($content) {
            $truncated = mb_substr($content, 0, 60);
            if (mb_strlen($content) > 60) {
                $truncated .= '...';
            }
            $this->update(['title' => $truncated]);
        }
    }
}
