<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;
use App\Traits\Auditable;

class UsulanKegiatan extends Model implements HasMedia
{
    use HasFactory, InteractsWithMedia, Auditable;

    protected $table = 'tbl_usulan_kegiatan';

    protected $fillable = [
        'user_id',
        'sub_bidang',
        'nama_pengusul',
        'kecamatan_id',
        'desa_id',
        'perihal',
        'ringkasan',
        'tanggal_surat_masuk',
        'nomor_surat_masuk',
        'tanggal_surat',
    ];

    protected $casts = [
        'tanggal_surat_masuk' => 'date',
        'tanggal_surat' => 'date',
    ];

    /**
     * Get the user who created this usulan.
     */
    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    /**
     * Get the kecamatan.
     */
    public function kecamatan(): BelongsTo
    {
        return $this->belongsTo(Kecamatan::class, 'kecamatan_id');
    }

    /**
     * Get the desa.
     */
    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    /**
     * Register media collections.
     */
    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('dokumen');
    }
}
