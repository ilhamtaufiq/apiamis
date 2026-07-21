<?php

namespace App\Models;

use App\Traits\Auditable;
use App\Traits\NotifiesAdminsOnChanges;
use Illuminate\Database\Eloquent\Factories\HasFactory;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class Kontrak extends Model implements HasMedia
{
    use Auditable, HasFactory, InteractsWithMedia, NotifiesAdminsOnChanges;

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
        'spse_sppbj_id',
        'spse_spk_id',
        'spse_rekanan_id',
        'spse_pushed_at',
        'spse_push_log',
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
        'spse_pushed_at' => 'datetime',
        'spse_push_log' => 'array',
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

    /**
     * Kontrak yang tertaut ke minimal satu paket lewat legacy id_pekerjaan ATAU pivot konsolidasi.
     * Callback opsional diterapkan ke query Pekerjaan (notCanceled, tahun, dll.).
     *
     * @param  callable(\Illuminate\Database\Eloquent\Builder): void|null  $pekerjaanConstraint
     */
    public function scopeLinkedToPekerjaan($query, ?callable $pekerjaanConstraint = null)
    {
        return $query->where(function ($outer) use ($pekerjaanConstraint) {
            $outer->whereHas('pekerjaan', function ($q) use ($pekerjaanConstraint) {
                if ($pekerjaanConstraint) {
                    $pekerjaanConstraint($q);
                }
            })->orWhereHas('pekerjaans', function ($q) use ($pekerjaanConstraint) {
                if ($pekerjaanConstraint) {
                    $pekerjaanConstraint($q);
                }
            });
        });
    }

    public function penyedia(): BelongsTo
    {
        return $this->belongsTo(Penyedia::class, 'id_penyedia');
    }

    public function progress_fisik(): \Illuminate\Database\Eloquent\Relations\HasOne
    {
        return $this->hasOne(PuspenProgressFisik::class, 'kontrak_id');
    }

    public function progress_fisik_outputs(): \Illuminate\Database\Eloquent\Relations\HasMany
    {
        return $this->hasMany(PuspenProgressFisikOutput::class, 'kontrak_id');
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

    public function spseNamaPaketIfDifferent(): ?string
    {
        $kodePaket = trim((string) $this->kode_paket);
        if ($kodePaket === '') {
            return null;
        }

        $spseNama = $this->sanitizeSpsePaketName((string) ProcurementStagingPaket::query()
            ->where('kode_paket', $kodePaket)
            ->orderByDesc('fetched_at')
            ->value('nama_paket'));

        if ($spseNama === '') {
            return null;
        }

        $spseNormalized = $this->normalizePaketNameForCompare($spseNama);
        $pekerjaans = $this->relationLoaded('pekerjaans')
            ? $this->pekerjaans
            : $this->pekerjaans()->get(['nama_paket']);

        foreach ($pekerjaans as $pekerjaan) {
            if ($this->normalizePaketNameForCompare($pekerjaan->nama_paket) === $spseNormalized) {
                return null;
            }
        }

        return $spseNama;
    }

    private function sanitizeSpsePaketName(string $value): string
    {
        $value = html_entity_decode($value, ENT_QUOTES | ENT_HTML5, 'UTF-8');
        $value = preg_replace('/\s+/u', ' ', strip_tags($value)) ?? $value;

        return trim($value);
    }

    private function normalizePaketNameForCompare(?string $value): string
    {
        $value = $this->sanitizeSpsePaketName((string) $value);
        $value = preg_replace('/[^\pL\pN]+/u', '', $value);

        return mb_strtolower($value ?? '');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('kontrak/ringkasan-preview')->singleFile();
    }
}
