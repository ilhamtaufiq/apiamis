<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class BerkasResource extends JsonResource
{
    public function toArray($request)
    {
        $media = $this->getFirstMedia('berkas/dokumen');

        return [
            'id' => $this->id,
            'jenis_dokumen' => $this->jenis_dokumen,
            'pekerjaan_id' => $this->pekerjaan_id,
            'uploaded_by' => $this->uploaded_by,
            'berkas_url' => $media?->getUrl() ?? $this->getFirstMediaUrl('berkas/dokumen'),
            'file_name' => $media?->file_name,
            'mime_type' => $media?->mime_type,
            'size' => $media?->size,
            'media_id' => $media?->id,
            'pekerjaan' => $this->whenLoaded('pekerjaan', function () {
                return [
                    'id' => $this->pekerjaan->id,
                    'nama_paket' => $this->pekerjaan->nama_paket,
                ];
            }),
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
