<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class KegiatanResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'nama_program' => $this->nama_program,
            'nama_kegiatan' => $this->nama_kegiatan,
            'nama_sub_kegiatan' => $this->nama_sub_kegiatan,
            'tahun_anggaran' => $this->tahun_anggaran,
            'sumber_dana' => $this->sumber_dana,
            'pagu' => $this->pagu,
            'kode_rekening' => $this->kode_rekening,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
