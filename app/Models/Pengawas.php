<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;

use App\Traits\Auditable;

class Pengawas extends Model
{
    use Auditable;

    protected $table = 'pengawas';

    protected $fillable = [
        'nama',
        'nip',
        'jabatan',
        'telepon'
    ];
}
