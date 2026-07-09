<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\Image\Enums\Fit;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\MediaCollections\Models\Media;
use Spatie\MediaLibrary\InteractsWithMedia;

use App\Traits\Auditable;
use App\Traits\BroadcastsPekerjaanRealtime;
use App\Traits\NotifiesAdminsOnChanges;

class Foto extends Model implements HasMedia
{
    use BroadcastsPekerjaanRealtime, InteractsWithMedia, NotifiesAdminsOnChanges, Auditable;

    protected $table = 'tbl_foto';

    protected $fillable = [
        'pekerjaan_id',
        'komponen_id',
        'penerima_id',
        'keterangan',
        'koordinat',
        'validasi_koordinat',
        'validasi_koordinat_message',
        'unit_index'
    ];

    protected $casts = [
        'pekerjaan_id' => 'integer',
        'komponen_id' => 'integer',
        'penerima_id' => 'integer',
        'validasi_koordinat' => 'boolean',
        'unit_index' => 'integer',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function penerima(): BelongsTo
    {
        return $this->belongsTo(Penerima::class, 'penerima_id');
    }

    public function komponen(): BelongsTo
    {
        return $this->belongsTo(Output::class, 'komponen_id');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('foto/pekerjaan');
    }

    public function registerMediaConversions(?Media $media = null): void
    {
        $this->addMediaConversion('thumb')
            ->fit(Fit::Crop, 120, 120)
            ->sharpen(10)
            ->nonQueued();
    }
}
