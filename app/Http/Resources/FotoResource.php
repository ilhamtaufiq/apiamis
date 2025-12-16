<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class FotoResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'pekerjaan_id' => $this->pekerjaan_id,
            'komponen_id' => $this->komponen_id,
            'penerima_id' => $this->penerima_id,
            'keterangan' => $this->keterangan,
            'koordinat' => $this->koordinat,
            'validasi_koordinat' => $this->validasi_koordinat,
            'validasi_koordinat_message' => $this->validasi_koordinat_message,
            'foto_url' => $this->getFirstMediaUrl('foto/pekerjaan'),
            'pekerjaan' => $this->whenLoaded('pekerjaan', function () {
                return [
                    'id' => $this->pekerjaan->id,
                    'nama_paket' => $this->pekerjaan->nama_paket,
                ];
            }),
            'penerima' => $this->whenLoaded('penerima', function () {
                return [
                    'id' => $this->penerima->id,
                    'nama' => $this->penerima->nama,
                    'nik' => $this->penerima->nik,
                ];
            }),
            'komponen' => $this->whenLoaded('komponen', function () {
                return [
                    'id' => $this->komponen->id,
                    'komponen' => $this->komponen->komponen,
                ];
            }),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
