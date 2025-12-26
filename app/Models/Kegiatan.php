<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

use App\Traits\NotifiesAdminsOnChanges;

class Kegiatan extends Model
{
    use NotifiesAdminsOnChanges;
    protected $table = 'tbl_kegiatan';
    
    protected $fillable = [
        'nama_program',
        'nama_kegiatan',
        'nama_sub_kegiatan',
        'tahun_anggaran',
        'sumber_dana',
        'pagu',
        'kode_rekening'
    ];

    protected $casts = [
        'pagu' => 'decimal:2',
        'kode_rekening' => 'array'
    ];
}
