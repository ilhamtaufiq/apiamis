<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class TiketResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     *
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'user_id' => $this->user_id,
            'user' => new UserResource($this->whenLoaded('user')),
            'pekerjaan_id' => $this->pekerjaan_id,
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'subjek' => $this->subjek,
            'deskripsi' => $this->deskripsi,
            'kategori' => $this->kategori,
            'prioritas' => $this->prioritas,
            'status' => $this->status,
            'admin_notes' => $this->admin_notes,
            'comments' => TiketCommentResource::collection($this->whenLoaded('comments')),
            'image_url' => $this->getFirstMediaUrl('attachment'),
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}
