<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Support\Str;

class SpamKelembagaanShareLink extends Model
{
    protected $table = 'spam_kelembagaan_share_links';

    protected $fillable = [
        'unit_spam_id',
        'created_by',
        'token',
        'label',
        'is_active',
        'expires_at',
        'max_submissions',
        'submission_count',
        'admin_note',
    ];

    protected $casts = [
        'is_active' => 'boolean',
        'expires_at' => 'datetime',
        'max_submissions' => 'integer',
        'submission_count' => 'integer',
    ];

    public function unitSpam(): BelongsTo
    {
        return $this->belongsTo(UnitSpam::class, 'unit_spam_id');
    }

    public function creator(): BelongsTo
    {
        return $this->belongsTo(User::class, 'created_by');
    }

    public function submissions(): HasMany
    {
        return $this->hasMany(SpamKelembagaanSubmission::class, 'share_link_id');
    }

    public static function generateToken(): string
    {
        return Str::lower(Str::random(48));
    }

    public function isUsable(): bool
    {
        if (! $this->is_active) {
            return false;
        }

        if ($this->expires_at && $this->expires_at->isPast()) {
            return false;
        }

        if ($this->max_submissions !== null && $this->submission_count >= $this->max_submissions) {
            return false;
        }

        return true;
    }

    public function publicUrlPath(): string
    {
        return '/kelembagaan-spam/form/'.$this->token;
    }
}
