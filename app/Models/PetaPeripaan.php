<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

use App\Traits\Auditable;

class PetaPeripaan extends Model implements HasMedia
{
    use InteractsWithMedia, Auditable;

    protected $table = 'tbl_peta_peripaan';

    protected $fillable = [
        'pekerjaan_id',
        'nama',
        'geojson',
        'uploaded_by',
    ];

    protected $casts = [
        'geojson' => 'array',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function uploader(): BelongsTo
    {
        return $this->belongsTo(User::class, 'uploaded_by');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('peripaan/kml');
    }
}
