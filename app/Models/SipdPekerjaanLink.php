<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class SipdPekerjaanLink extends Model
{
    protected $table = 'tbl_sipd_pekerjaan_links';

    protected $fillable = [
        'id_sub_bl',
        'id_rinci_sub_bl',
        'pekerjaan_id',
    ];

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }
}
