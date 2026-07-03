<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

class ProcurementSyncRun extends Model
{
    public $timestamps = false;

    protected $table = 'tbl_procurement_sync_runs';

    protected $fillable = [
        'user_id',
        'status',
        'item_count',
        'matched_count',
        'error_log',
        'started_at',
        'finished_at',
    ];

    protected $casts = [
        'started_at' => 'datetime',
        'finished_at' => 'datetime',
    ];

    public function user(): BelongsTo
    {
        return $this->belongsTo(User::class);
    }

    public function stagingPakets(): HasMany
    {
        return $this->hasMany(ProcurementStagingPaket::class, 'sync_run_id');
    }
}