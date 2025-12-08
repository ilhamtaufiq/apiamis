<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PenyediaResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'nama' => $this->nama,
            'direktur' => $this->direktur,
            'no_akta' => $this->no_akta,
            'notaris' => $this->notaris,
            'tanggal_akta' => $this->tanggal_akta?->format('Y-m-d'),
            'alamat' => $this->alamat,
            'bank' => $this->bank,
            'norek' => $this->norek,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
