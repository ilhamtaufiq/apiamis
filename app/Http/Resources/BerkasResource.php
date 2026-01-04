<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class BerkasResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'jenis_dokumen' => $this->jenis_dokumen,
            'pekerjaan_id' => $this->pekerjaan_id,
            'berkas_url' => $this->getFirstMediaUrl('berkas/dokumen'),
            'pekerjaan' => $this->whenLoaded('pekerjaan', function () {
                return [
                    'id' => $this->pekerjaan->id,
                    'nama_paket' => $this->pekerjaan->nama_paket,
                ];
            }),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
