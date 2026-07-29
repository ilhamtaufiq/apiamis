<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class UsulanKegiatanResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     *
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'user_id' => $this->user_id,
            'user' => new UserResource($this->whenLoaded('user')),
            'sub_bidang' => $this->sub_bidang,
            'nama_pengusul' => $this->nama_pengusul,
            'kecamatan_id' => $this->kecamatan_id,
            'kecamatan' => new KecamatanResource($this->whenLoaded('kecamatan')),
            'desa_id' => $this->desa_id,
            'desa' => new DesaResource($this->whenLoaded('desa')),
            'perihal' => $this->perihal,
            'ringkasan' => $this->ringkasan,
            'dokumen_url' => $this->getFirstMediaUrl('dokumen'),
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}
