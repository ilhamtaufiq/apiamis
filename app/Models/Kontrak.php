<?php

namespace App\Models;

use App\Traits\Auditable;
use App\Traits\NotifiesAdminsOnChanges;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class Kontrak extends Model
{
    use Auditable, HasFactory, NotifiesAdminsOnChanges;

    protected $table = 'tbl_kontrak';

    protected $fillable = [
        'id_kegiatan',
        'id_pekerjaan',
        'id_penyedia',
        'kode_rup',
        'kode_paket',
        'nomor_penawaran',
        'tanggal_penawaran',
        'nilai_kontrak',
        'tgl_sppbj',
        'tgl_spk',
        'tgl_spmk',
        'tgl_selesai',
        'sppbj',
        'spk',
        'spmk',
    ];

    protected $casts = [
        'id_kegiatan' => 'integer',
        'id_pekerjaan' => 'integer',
        'id_penyedia' => 'integer',
        'tanggal_penawaran' => 'date',
        'tgl_sppbj' => 'date',
        'tgl_spk' => 'date',
        'tgl_spmk' => 'date',
        'tgl_selesai' => 'date',
        'nilai_kontrak' => 'float',
    ];

    public function kegiatan(): BelongsTo
    {
        return $this->belongsTo(Kegiatan::class, 'id_kegiatan');
    }

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'id_pekerjaan');
    }

    public function pekerjaans(): \Illuminate\Database\Eloquent\Relations\BelongsToMany
    {
        return $this->belongsToMany(Pekerjaan::class, 'kontrak_pekerjaan', 'kontrak_id', 'pekerjaan_id')
                    ->withTimestamps();
    }

    public function penyedia(): BelongsTo
    {
        return $this->belongsTo(Penyedia::class, 'id_penyedia');
    }

    public function progress_fisik(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(PuspenProgressFisik::class, 'kontrak_id');
    }

    public function registers(): \Illuminate\Database\Eloquent\Relations\HasMany
    {
        return $this->hasMany(DocumentRegister::class, 'kontrak_id');
    }

    public function addendums(): \Illuminate\Database\Eloquent\Relations\HasMany
    {
        return $this->hasMany(KontrakAddendum::class, 'kontrak_id')->orderBy('addendum_ke');
    }

    public function approvedAddendums(): \Illuminate\Database\Eloquent\Relations\HasMany
    {
        return $this->addendums()->where('status', 'disetujui');
    }

    public function latestApprovedAddendum(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(KontrakAddendum::class, 'kontrak_id')
            ->where('status', 'disetujui')
            ->latestOfMany('addendum_ke');
    }

    public function nilaiKontrakBerjalan(): ?float
    {
        $addendum = $this->relationLoaded('latestApprovedAddendum')
            ? $this->latestApprovedAddendum
            : $this->latestApprovedAddendum()->first();

        return $addendum?->nilai_kontrak_sesudah ?? $this->nilai_kontrak;
    }

    public function tglSelesaiBerjalan(): mixed
    {
        $addendum = $this->relationLoaded('latestApprovedAddendum')
            ? $this->latestApprovedAddendum
            : $this->latestApprovedAddendum()->first();

        return $addendum?->tgl_selesai_sesudah ?? $this->tgl_selesai;
    }
}
