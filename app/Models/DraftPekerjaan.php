<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use App\Traits\Auditable;

class DraftPekerjaan extends Model
{
    use Auditable;
    protected $table = 'tbl_draft_pekerjaan';

    protected $fillable = [
        'pekerjaan_id',
        'penyedia_id',
        'nama_pelaksana',
        'kode_rup',
        'kode_paket',
    ];

    public function pekerjaan()
    {
        return $this->belongsTo(Pekerjaan::class, 'pekerjaan_id');
    }

    public function penyedia()
    {
        return $this->belongsTo(Penyedia::class, 'penyedia_id');
    }
}
