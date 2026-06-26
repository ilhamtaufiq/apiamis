<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class KanbanBoardResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'slug' => $this->slug,
            'title' => $this->title,
            'description' => $this->description,
            'columns' => KanbanColumnResource::collection($this->whenLoaded('columns')),
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}