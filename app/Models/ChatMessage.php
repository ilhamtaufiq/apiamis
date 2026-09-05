<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class ChatMessage extends Model
{
    protected $fillable = ['chat_session_id', 'role', 'content', 'tool_calls', 'tokens_used', 'cost_idr'];

    protected $casts = [
        'tool_calls' => 'array',
        'cost_idr' => 'float',
    ];

    public function session(): BelongsTo
    {
        return $this->belongsTo(ChatSession::class, 'chat_session_id');
    }
}
