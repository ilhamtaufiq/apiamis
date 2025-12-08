<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class OutputResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'pekerjaan_id' => $this->pekerjaan_id,
            'komponen' => $this->komponen,
            'satuan' => $this->satuan,
            'volume' => $this->volume,
            'penerima_is_optional' => $this->penerima_is_optional,
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
