<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class PetaPeripaanResource extends JsonResource
{
    public function toArray($request)
    {
        $media = $this->getFirstMedia('peripaan/kml');

        return [
            'id' => $this->id,
            'nama' => $this->nama,
            'pekerjaan_id' => $this->pekerjaan_id,
            'geojson' => $this->geojson,
            'uploaded_by' => $this->uploaded_by,
            'file_url' => $media?->getUrl(),
            'file_name' => $media?->file_name,
            'size' => $media?->size,
            'media_id' => $media?->id,
            'pekerjaan' => $this->whenLoaded('pekerjaan', function () {
                return $this->pekerjaan ? [
                    'id' => $this->pekerjaan->id,
                    'nama_paket' => $this->pekerjaan->nama_paket,
                ] : null;
            }),
            'uploader' => $this->whenLoaded('uploader', function () {
                return $this->uploader ? [
                    'id' => $this->uploader->id,
                    'name' => $this->uploader->name,
                ] : null;
            }),
            'created_at' => $this->created_at?->toIso8601String(),
        ];
    }
}
