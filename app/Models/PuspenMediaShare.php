<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\SoftDeletes;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class PuspenMediaShare extends Model implements HasMedia
{
    use Auditable, InteractsWithMedia, SoftDeletes;

    protected $fillable = [
        'user_id',
        'title',
        'description',
        'share_token',
        'is_public',
        'expires_at',
        'download_count',
        'last_downloaded_at',
    ];

    protected $casts = [
        'is_public' => 'boolean',
        'expires_at' => 'datetime',
        'last_downloaded_at' => 'datetime',
    ];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('shared-media');
    }

    public function scopeOwnedBy($query, int $userId)
    {
        return $query->where('user_id', $userId);
    }

    public function canManage(User $user): bool
    {
        return $this->user_id === $user->id || $user->hasRole('admin');
    }

    public function isDownloadable(): bool
    {
        if (! $this->is_public) {
            return false;
        }

        if ($this->expires_at && $this->expires_at->isPast()) {
            return false;
        }

        return $this->getMedia('shared-media')->isNotEmpty();
    }
}
