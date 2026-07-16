<?php

namespace App\Models;

use App\Traits\Auditable;
use App\Traits\NotifiesAdminsOnChanges;
use Illuminate\Database\Eloquent\Model;

class Kegiatan extends Model
{
    use Auditable, NotifiesAdminsOnChanges;

    protected $table = 'tbl_kegiatan';

    public const SUMBER_DANA_OPTIONS = [
        'APBD',
        'APBN',
        'DAU',
        'DAK',
        'DID',
        'Bantuan Provinsi',
        'DBH',
        'SILPA',
        'DBH Pajak Rokok',
        'PAD',
        'DBHCT',
        'DBH Prov',
    ];

    protected $fillable = [
        'nama_program',
        'sub_bidang',
        'nama_kegiatan',
        'nama_sub_kegiatan',
        'tahun_anggaran',
        'sumber_dana',
        'pagu',
        'kode_rekening',
        'nama_pptk',
        'nip_pptk',
        'sipd_id_sub_bl',
        'kode_sub_giat',
    ];

    protected $casts = [
        'pagu' => 'decimal:2',
        'kode_rekening' => 'array',
        'sipd_id_sub_bl' => 'integer',
    ];
}
