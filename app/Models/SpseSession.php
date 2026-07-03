<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class SpseSession extends Model
{
    protected $table = 'tbl_spse_sessions';

    protected $fillable = [
        'user_id',
        'encrypted_cookies',
        'lpse_slug',
        'expires_at',
        'last_validated_at',
        'is_active',
    ];

    protected $casts = [
        'encrypted_cookies' => 'encrypted:array',
        'expires_at' => 'datetime',
        'last_validated_at' => 'datetime',
        'is_active' => 'boolean',
    ];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    public function scopeActiveForUser($query, int $userId)
    {
        return $query
            ->where('user_id', $userId)
            ->where('is_active', true)
            ->where(function ($q) {
                $q->whereNull('expires_at')->orWhere('expires_at', '>', now());
            })
            ->latest('id');
    }
}