<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

use App\Traits\NotifiesAdminsOnChanges;

class Penyedia extends Model
{
    use NotifiesAdminsOnChanges;
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
