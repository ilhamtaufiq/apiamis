<?php

namespace App\Models;

use App\Traits\Auditable;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;
use Spatie\MediaLibrary\HasMedia;
use Spatie\MediaLibrary\InteractsWithMedia;

class KontrakAddendum extends Model implements HasMedia
{
    use Auditable, InteractsWithMedia;

    protected $table = 'tbl_kontrak_addendums';

    protected $fillable = [
        'kontrak_id',
        'addendum_ke',
        'nomor_addendum',
        'tanggal_addendum',
        'jenis_addendum',
        'alasan',
        'deskripsi_perubahan',
        'nilai_kontrak_sebelum',
        'nilai_kontrak_sesudah',
        'tgl_selesai_sebelum',
        'tgl_selesai_sesudah',
        'status',
        'kelengkapan_override',
        'created_by',
        'approved_by',
        'approved_at',
    ];

    protected $casts = [
        'tanggal_addendum' => 'date',
        'tgl_selesai_sebelum' => 'date',
        'tgl_selesai_sesudah' => 'date',
        'nilai_kontrak_sebelum' => 'float',
        'nilai_kontrak_sesudah' => 'float',
        'approved_at' => 'datetime',
        'kelengkapan_override' => 'boolean',
    ];

    public function kontrak(): BelongsTo
    {
        return $this->belongsTo(Kontrak::class, 'kontrak_id');
    }

    public function items(): HasMany
    {
        return $this->hasMany(KontrakAddendumItem::class, 'addendum_id');
    }

    public function creator(): BelongsTo
    {
        return $this->belongsTo(User::class, 'created_by');
    }

    public function approver(): BelongsTo
    {
        return $this->belongsTo(User::class, 'approved_by');
    }

    public function registerMediaCollections(): void
    {
        $this->addMediaCollection('kontrak/addendum');
    }
}
