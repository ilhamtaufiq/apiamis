<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

use App\Traits\Auditable;

class Sk extends Model implements HasMedia
{
    use InteractsWithMedia, Auditable;

    protected $table = 'sk';

    protected $fillable = [
        'nomor_sk',
        'nama',
        'tanggal_sk',
        'uploaded_by',
    ];

    protected $casts = [
        'tanggal_sk' => 'date',
    ];

    public function uploader(): BelongsTo
    {
        return $this->belongsTo(User::class, 'uploaded_by');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('sk/dokumen');
    }
}
