<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class KontrakAddendumItem extends Model
{
    protected $table = 'tbl_kontrak_addendum_items';

    protected $fillable = [
        'addendum_id',
        'nama_item',
        'spesifikasi_sebelum',
        'spesifikasi_sesudah',
        'volume_sebelum',
        'volume_sesudah',
        'harga_sebelum',
        'harga_sesudah',
        'subtotal_sebelum',
        'subtotal_sesudah',
    ];

    protected $casts = [
        'volume_sebelum' => 'float',
        'volume_sesudah' => 'float',
        'harga_sebelum' => 'float',
        'harga_sesudah' => 'float',
        'subtotal_sebelum' => 'float',
        'subtotal_sesudah' => 'float',
    ];

    public function addendum(): BelongsTo
    {
        return $this->belongsTo(KontrakAddendum::class, 'addendum_id');
    }
}
