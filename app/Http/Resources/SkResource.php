<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class SkResource extends JsonResource
{
    public function toArray($request)
    {
        $media = $this->getFirstMedia('sk/dokumen');

        return [
            'id' => $this->id,
            'nomor_sk' => $this->nomor_sk,
            'nama' => $this->nama,
            'tanggal_sk' => $this->tanggal_sk?->toDateString(),
            'uploaded_by' => $this->uploaded_by,
            'file_url' => $media?->getUrl(),
            'file_name' => $media?->file_name,
            'mime_type' => $media?->mime_type,
            'size' => $media?->size,
            'media_id' => $media?->id,
            'uploader' => $this->whenLoaded('uploader', function () {
                return $this->uploader ? [
                    'id' => $this->uploader->id,
                    'name' => $this->uploader->name,
                    'email' => $this->uploader->email,
                ] : null;
            }),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
