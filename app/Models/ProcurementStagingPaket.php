<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class ProcurementStagingPaket extends Model
{
    public $timestamps = false;

    protected $table = 'tbl_procurement_staging_paket';

    protected $fillable = [
        'sync_run_id',
        'sumber',
        'kode_paket',
        'nama_paket',
        'status_paket',
        'metode_pengadaan',
        'jenis_paket',
        'matched_pekerjaan_id',
        'matched_kontrak_id',
        'match_status',
        'raw_row',
        'fetched_at',
    ];

    protected $casts = [
        'raw_row' => 'array',
        'fetched_at' => 'datetime',
    ];

    public function syncRun(): BelongsTo
    {
        return $this->belongsTo(ProcurementSyncRun::class, 'sync_run_id');
    }

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'matched_pekerjaan_id');
    }

    public function kontrak(): BelongsTo
    {
        return $this->belongsTo(Kontrak::class, 'matched_kontrak_id');
    }
}