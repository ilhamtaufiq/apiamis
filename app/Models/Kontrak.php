<?php

namespace App\Models;

use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Factories\HasFactory;

use App\Traits\NotifiesAdminsOnChanges;
use App\Traits\Auditable;

class Kontrak extends Model
{
    use NotifiesAdminsOnChanges, HasFactory, Auditable;
    protected $table = 'tbl_kontrak';

    protected $fillable = [
        'id_kegiatan',
        'id_pekerjaan',
        'id_penyedia',
        'kode_rup',
        'kode_paket',
        'nomor_penawaran',
        'tanggal_penawaran',
        'nilai_kontrak',
        'tgl_sppbj',
        'tgl_spk',
        'tgl_spmk',
        'tgl_selesai',
        'sppbj',
        'spk',
        'spmk'
    ];

    protected $casts = [
        'id_kegiatan' => 'integer',
        'id_pekerjaan' => 'integer',
        'id_penyedia' => 'integer',
        'tanggal_penawaran' => 'date',
        'tgl_sppbj' => 'date',
        'tgl_spk' => 'date',
        'tgl_spmk' => 'date',
        'tgl_selesai' => 'date',
        'nilai_kontrak' => 'float',
    ];

    public function kegiatan(): BelongsTo
    {
        return $this->belongsTo(Kegiatan::class, 'id_kegiatan');
    }

    public function pekerjaan(): BelongsTo
    {
        return $this->belongsTo(Pekerjaan::class, 'id_pekerjaan');
    }

    public function penyedia(): BelongsTo
    {
        return $this->belongsTo(Penyedia::class, 'id_penyedia');
    }
}
