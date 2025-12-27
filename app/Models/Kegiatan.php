<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Kegiatan extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
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
