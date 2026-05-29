<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;

class RkaItem extends Model
{
    protected $table = 'tbl_rka_items';

    protected $fillable = [
        'rka_document_id',
        'kode_rekening',
        'tipe',
        'uraian',
        'sumber_dana',
        'koefisien',
        'satuan',
        'harga',
        'jumlah',
        'jumlah_sebelum',
        'jumlah_setelah',
        'selisih',
        'raw_line',
        'sort_order',
    ];

    protected $casts = [
        'harga' => 'float',
        'jumlah' => 'float',
        'jumlah_sebelum' => 'float',
        'jumlah_setelah' => 'float',
        'selisih' => 'float',
    ];

    public function document(): BelongsTo
    {
        return $this->belongsTo(RkaDocument::class, 'rka_document_id');
    }
}
