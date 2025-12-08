<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

class Penyedia extends Model
{
    protected $table = 'tbl_penyedia';
    
    protected $fillable = [
        'nama',
        'direktur',
        'no_akta',
        'notaris',
        'tanggal_akta',
        'alamat',
        'bank',
        'norek'
    ];

    protected $casts = [
        'tanggal_akta' => 'date'
    ];
}
