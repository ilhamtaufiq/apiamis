<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class KanbanCardResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'board_id' => $this->board_id,
            'column_id' => $this->column_id,
            'position' => $this->position,
            'title' => $this->title,
            'description' => $this->description,
            'status_label' => $this->status_label,
            'pekerjaan_id' => $this->pekerjaan_id,
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'tiket_id' => $this->tiket_id,
            'tiket' => new TiketResource($this->whenLoaded('tiket')),
            'source' => $this->source,
            'metadata' => $this->metadata,
            'created_by' => $this->created_by,
            'creator' => new UserResource($this->whenLoaded('creator')),
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}