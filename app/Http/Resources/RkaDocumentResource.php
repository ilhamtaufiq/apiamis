<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class RkaDocumentResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'jenis' => $this->jenis,
            'nama_file' => $this->nama_file,
            'nomor_dokumen' => $this->nomor_dokumen,
            'tahun_anggaran' => $this->tahun_anggaran,
            'program' => $this->program,
            'kegiatan' => $this->kegiatan,
            'sub_kegiatan' => $this->sub_kegiatan,
            'sumber_pendanaan' => $this->sumber_pendanaan ?? [],
            'total_sebelum' => $this->total_sebelum,
            'total_setelah' => $this->total_setelah,
            'total_selisih' => $this->total_selisih,
            'items_count' => $this->whenCounted('items'),
            'items' => RkaItemResource::collection($this->whenLoaded('items')),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
