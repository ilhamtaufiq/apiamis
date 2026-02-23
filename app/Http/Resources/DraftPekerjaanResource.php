<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class DraftPekerjaanResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'pekerjaan_id' => $this->pekerjaan_id,
            'penyedia_id' => $this->penyedia_id,
            'nama_pelaksana' => $this->nama_pelaksana,
            'kode_rup' => $this->kode_rup,
            'kode_paket' => $this->kode_paket,
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'penyedia' => new PenyediaResource($this->whenLoaded('penyedia')),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
