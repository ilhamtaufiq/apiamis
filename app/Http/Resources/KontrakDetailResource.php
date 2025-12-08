<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class KontrakDetailResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'kode_rup' => $this->kode_rup,
            'kode_paket' => $this->kode_paket,
            'nomor_penawaran' => $this->nomor_penawaran,
            'tanggal_penawaran' => $this->tanggal_penawaran?->format('Y-m-d'),
            'nilai_kontrak' => $this->nilai_kontrak,
            'tgl_sppbj' => $this->tgl_sppbj?->format('Y-m-d'),
            'tgl_spk' => $this->tgl_spk?->format('Y-m-d'),
            'tgl_spmk' => $this->tgl_spmk?->format('Y-m-d'),
            'tgl_selesai' => $this->tgl_selesai?->format('Y-m-d'),
            'sppbj' => $this->sppbj,
            'spk' => $this->spk,
            'spmk' => $this->spmk,
            'kegiatan' => new KegiatanResource($this->whenLoaded('kegiatan')),
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'penyedia' => new PenyediaResource($this->whenLoaded('penyedia')),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
