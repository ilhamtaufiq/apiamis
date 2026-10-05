<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class SurveyLokasi extends Model implements HasMedia
{
    use HasFactory, InteractsWithMedia, Auditable;

    protected $table = 'tbl_survey_lokasi';

    protected $fillable = [
        'user_id',
        'tugas_id',
        'jenis',
        'nama_lokasi',
        'kecamatan_id',
        'desa_id',
        'alamat',
        'latitude',
        'longitude',
        'detail',
        'status',
        'catatan_verifikasi',
        'verified_by',
        'verified_at',
    ];

    protected $casts = [
        'detail' => 'array',
        'verified_at' => 'datetime',
    ];

    /**
     * Surveyor/pengaju survei.
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class, 'user_id');
    }

    /**
     * Admin verifikator.
     */
    public function verifier(): BelongsTo
    {
        return $this->belongsTo(User::class, 'verified_by');
    }

    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    public function tugas(): BelongsTo
    {
        return $this->belongsTo(SurveyTugas::class, 'tugas_id');
    }

    /**
     * Register media collections.
     */
    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('foto');
    }
}
