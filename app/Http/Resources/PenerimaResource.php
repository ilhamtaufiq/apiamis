<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class PenerimaResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'nama' => $this->nama,
            'jumlah_jiwa' => $this->jumlah_jiwa,
            'nik' => $this->nik,
            'alamat' => $this->alamat,
            'is_komunal' => $this->is_komunal,
            'pekerjaan_id' => $this->pekerjaan_id,
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'created_at' => $this->created_at?->format('Y-m-d H:i:s'),
            'updated_at' => $this->updated_at?->format('Y-m-d H:i:s'),
        ];
    }
}
