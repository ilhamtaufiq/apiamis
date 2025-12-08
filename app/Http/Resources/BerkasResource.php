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
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
