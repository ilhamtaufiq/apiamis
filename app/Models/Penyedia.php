<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Penyedia extends Model
{
    use NotifiesAdminsOnChanges, Auditable;
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
