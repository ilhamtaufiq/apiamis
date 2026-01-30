<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\HasMany;

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

    /**
     * Pekerjaan yang diawasi sebagai pengawas utama
     */
    public function pekerjaanAsPengawas(): HasMany
    {
        return $this->hasMany(Pekerjaan::class, 'pengawas_id');
    }

    /**
     * Pekerjaan yang diawasi sebagai pendamping
     */
    public function pekerjaanAsPendamping(): HasMany
    {
        return $this->hasMany(Pekerjaan::class, 'pendamping_id');
    }
}
