<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class SpmAirMinumSource extends Model
{
    protected $table = 'spm_air_minum_sources';

    protected $guarded = [];

    public function spmAirMinum(): BelongsTo
    {
        return $this->belongsTo(SpmAirMinum::class, 'spm_air_minum_id');
    }
}
