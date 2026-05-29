<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\HasMany;

class RkaDocument extends Model
{
    protected $table = 'tbl_rka_documents';

    protected $fillable = [
        'jenis',
        'nama_file',
        'path_file',
        'path_text',
        'nomor_dokumen',
        'tahun_anggaran',
        'program',
        'kegiatan',
        'sub_kegiatan',
        'sumber_pendanaan',
        'total_sebelum',
        'total_setelah',
        'total_selisih',
        'imported_by',
    ];

    protected $casts = [
        'sumber_pendanaan' => 'array',
        'total_sebelum' => 'float',
        'total_setelah' => 'float',
        'total_selisih' => 'float',
    ];

    public function items(): HasMany
    {
        return $this->hasMany(RkaItem::class, 'rka_document_id')->orderBy('sort_order');
    }

    public function importer(): BelongsTo
    {
        return $this->belongsTo(User::class, 'imported_by');
    }
}
