<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Illuminate\Database\Eloquent\SoftDeletes;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;
use App\Models\ToolPdfSignaturePlacement;

class ToolPdf extends Model implements HasMedia
{
    use Auditable, InteractsWithMedia, SoftDeletes;

    protected $table = 'tool_pdfs';

    protected $fillable = [
        'user_id',
        'parent_id',
        'name',
        'original_filename',
        'kind',
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

    public function signaturePlacements(): HasMany
    {
        return $this->hasMany(ToolPdfSignaturePlacement::class, 'tool_pdf_id');
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
        $this->addMediaCollection('pdf')->singleFile();
    }

    public function getPdfUrlAttribute(): string
    {
        return $this->getFirstMediaUrl('pdf');
    }

    public function getHasPdfAttribute(): bool
    {
        return (bool) $this->getFirstMedia('pdf');
    }
}
