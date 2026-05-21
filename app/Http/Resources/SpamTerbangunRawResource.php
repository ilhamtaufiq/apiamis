<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class SpamTerbangunRawResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'kecamatan' => $this->kecamatan,
            'jenis_wilayah' => $this->jenis_wilayah,
            'desa_kelurahan' => $this->desa_kelurahan,
            'nama_pengelola' => $this->nama_pengelola,
            'sumber_air_baku' => $this->sumber_air_baku,
            'sistem_aliran' => $this->sistem_aliran,
            'debit_sumber_l_det' => $this->debit_sumber_l_det,
            'debit_diambil_l_det' => $this->debit_diambil_l_det,
            'penduduk_terlayani' => $this->penduduk_terlayani,
            'jumlah_penduduk' => $this->jumlah_penduduk,
            'hu_ku_unit' => $this->hu_ku_unit,
            'sr_unit' => $this->sr_unit,
            'tanpa_meteran_air_unit' => $this->tanpa_meteran_air_unit,
            'sumber_dana_raw' => $this->sumber_dana_raw,
            'asal_proyek' => $this->asal_proyek,
            'nilai_dak_apbn_rp' => $this->nilai_dak_apbn_rp,
            'nilai_apbd_rp' => $this->nilai_apbd_rp,
            'nilai_banprov_rp' => $this->nilai_banprov_rp,
            'tahun_pembangunan_raw' => $this->tahun_pembangunan_raw,
            'tahun_pembangunan_awal' => $this->tahun_pembangunan_awal,
            'tahun_pembangunan_akhir' => $this->tahun_pembangunan_akhir,
            'kondisi_raw' => $this->kondisi_raw,
            'kondisi_normalized' => $this->kondisi_normalized,
            'tanggal_terakhir_laporan' => $this->tanggal_terakhir_laporan?->toDateString(),
            'keterangan' => $this->keterangan,
            'raw_payload' => $this->raw_payload,
            'source_file' => $this->source_file,
            'source_sheet' => $this->source_sheet,
            'source_row' => $this->source_row,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
