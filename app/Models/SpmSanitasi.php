<?php

namespace App\Models;

use App\Traits\Auditable;
use App\Traits\NotifiesAdminsOnChanges;
use Illuminate\Database\Eloquent\Model;
use Illuminate\Database\Eloquent\Relations\BelongsTo;
use Illuminate\Database\Eloquent\Relations\BelongsToMany;

class SpmSanitasi extends Model
{
    use Auditable, NotifiesAdminsOnChanges;

    protected $table = 'tbl_spm_sanitasi';

    protected $fillable = [
        'jenis',
        'desa_id',
        'skala_pelayanan',
        'nama_infrastruktur',
        'latitude',
        'longitude',
        'alamat_lengkap',
        'jumlah_pemanfaat_kk',
        'jumlah_pemanfaat_jiwa',
        'tahun_konstruksi',
        'pembiayaan_apbn',
        'pembiayaan_apbd',
        'pembiayaan_dak',
        'pembiayaan_hibah',
        'pembiayaan_csr',
        'pembiayaan_lain',
        'pembiayaan_total',
        'status_keberfungsian',
        'kualitas_keberfungsian',
        'pengelola',
        'kapasitas_desain',
        'kapasitas_terpakai',
        'kapasitas_tidak_terpakai',
        'jenis_pengolahan',
        'peta_cakupan',
        'status_lahan',
        'luas_lahan_ha',
        'opsi_teknologi',
        'jumlah_stasiun_pompa',
        'biaya_operasional',
        'jenis_pengelola',
        'sistem_pengolahan',
        'truk_tinja_unit',
        'kapasitas_truk_m3',
        'jumlah_ritasi',
        'jarak_maksimal_pelayanan_km',
        'alokasi_biaya_operasional',
    ];

    protected $casts = [
        'latitude' => 'float',
        'longitude' => 'float',
        'jumlah_pemanfaat_kk' => 'integer',
        'jumlah_pemanfaat_jiwa' => 'integer',
        'tahun_konstruksi' => 'integer',
        'pembiayaan_apbn' => 'float',
        'pembiayaan_apbd' => 'float',
        'pembiayaan_dak' => 'float',
        'pembiayaan_hibah' => 'float',
        'pembiayaan_csr' => 'float',
        'pembiayaan_lain' => 'float',
        'pembiayaan_total' => 'float',
        'kapasitas_desain' => 'float',
        'kapasitas_terpakai' => 'float',
        'kapasitas_tidak_terpakai' => 'float',
        'biaya_operasional' => 'float',
        'truk_tinja_unit' => 'integer',
        'kapasitas_truk_m3' => 'float',
        'jumlah_ritasi' => 'integer',
        'jarak_maksimal_pelayanan_km' => 'float',
        'alokasi_biaya_operasional' => 'float',
    ];

    public function desa(): BelongsTo
    {
        return $this->belongsTo(Desa::class, 'desa_id');
    }

    public function pekerjaan(): BelongsToMany
    {
        return $this->belongsToMany(Pekerjaan::class, 'tbl_spm_sanitasi_pekerjaan', 'spm_sanitasi_id', 'pekerjaan_id')
            ->withPivot('output_id')
            ->withTimestamps();
    }
}