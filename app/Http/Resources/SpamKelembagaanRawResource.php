<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class SpamKelembagaanRawResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'jenis_jaringan' => $this->jenis_jaringan,
            'kecamatan' => $this->kecamatan,
            'desa_kelurahan' => $this->desa_kelurahan,
            'desa_kelurahan_normalized' => $this->desa_kelurahan_normalized,
            'lokasi_key' => $this->lokasi_key,
            'tahun_pembangunan_raw' => $this->tahun_pembangunan_raw,
            'tahun_pembangunan_awal' => $this->tahun_pembangunan_awal,
            'tahun_pembangunan_akhir' => $this->tahun_pembangunan_akhir,
            'sumber_dana_raw' => $this->sumber_dana_raw,
            'program_pembangunan' => $this->program_pembangunan,
            'nama_pengelola' => $this->nama_pengelola,
            'perdes_pembentukan_pokmas' => $this->perdes_pembentukan_pokmas,
            'pengurus_kepala' => $this->pengurus_kepala,
            'pengurus_bendahara' => $this->pengurus_bendahara,
            'pengurus_sekretaris' => $this->pengurus_sekretaris,
            'kapasitas_mata_air_l_det' => $this->kapasitas_mata_air_l_det,
            'sistem_aliran' => $this->sistem_aliran,
            'kapasitas_air_tanah_l_det' => $this->kapasitas_air_tanah_l_det,
            'kapasitas_lain_l_det' => $this->kapasitas_lain_l_det,
            'dasar_hukum_tarif' => $this->dasar_hukum_tarif,
            'besaran_iuran' => $this->besaran_iuran,
            'pendapatan_bulanan_rp' => $this->pendapatan_bulanan_rp,
            'biaya_operasional_bulanan_rp' => $this->biaya_operasional_bulanan_rp,
            'sr_unit' => $this->sr_unit,
            'kk_terlayani' => $this->kk_terlayani,
            'jiwa_terlayani' => $this->jiwa_terlayani,
            'target_layanan' => $this->target_layanan,
            'raw_payload' => $this->raw_payload,
            'source_file' => $this->source_file,
            'source_sheet' => $this->source_sheet,
            'source_row' => $this->source_row,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
