<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class KecamatanDetailResource extends JsonResource
{
    /**
     * Transform the resource into an array (dengan relasi desa).
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'nama_kecamatan' => $this->n_kec,
            'desa' => DesaResource::collection($this->desa),
            'jumlah_desa' => $this->desa->count(),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
