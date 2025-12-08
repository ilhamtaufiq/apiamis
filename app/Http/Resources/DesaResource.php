<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class DesaResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'nama_desa' => $this->n_desa,
            'luas' => $this->luas,
            'jumlah_penduduk' => $this->jumlah_penduduk,
            'kecamatan_id' => $this->kecamatan_id,
            'kecamatan' => new KecamatanResource($this->whenLoaded('kecamatan')),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
