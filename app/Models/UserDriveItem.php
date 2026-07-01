<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\SoftDeletes;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class UserDriveItem extends Model implements HasMedia
{
    use Auditable, InteractsWithMedia, SoftDeletes;

    public const KIND_FOLDER = 'folder';

    public const KIND_FILE = 'file';

    protected $fillable = [
        'user_id',
        'parent_id',
        'name',
        'kind',
        'original_filename',
    ];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }

    public function parent(): BelongsTo
    {
        return $this->belongsTo(self::class, 'parent_id');
    }

    public function children(): HasMany
    {
        return $this->hasMany(self::class, 'parent_id');
    }

    public function scopeOwnedBy($query, int $userId)
    {
        return $query->where('user_id', $userId);
    }

    public function canManage(User $user): bool
    {
        return $this->user_id === $user->id || $user->hasRole('admin');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('drive-file')->singleFile();
    }

    public function getFileUrlAttribute(): ?string
    {
        $media = $this->getFirstMedia('drive-file');

        return $media?->getFullUrl();
    }

    public function getMimeTypeAttribute(): ?string
    {
        $media = $this->getFirstMedia('drive-file');

        return $media?->mime_type;
    }

    public function getFileSizeAttribute(): ?int
    {
        $media = $this->getFirstMedia('drive-file');

        return $media?->size;
    }
}