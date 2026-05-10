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
     * Auto-generate title from first user message
     */
    public function generateTitle(): void
    {
        $firstMessage = $this->messages()->where('role', 'user')->first();
        if ($firstMessage) {
            $this->update([
                'title' => mb_substr($firstMessage->content, 0, 60) . (mb_strlen($firstMessage->content) > 60 ? '...' : '')
            ]);
        }
    }
}
