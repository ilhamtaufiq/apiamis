<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class MasterFasePekerjaan extends Model
{
    protected $fillable = [
        'jenis_proyek',
        'kode_fase',
        'nama_fase',
        'prioritas',
        'overlap_persen',
        'durasi_faktor',
        'keywords',
        'deskripsi',
        'is_active',
    ];

    protected $casts = [
        'keywords' => 'array',
        'is_active' => 'boolean',
    ];
}
