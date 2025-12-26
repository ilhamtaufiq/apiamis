<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class EventResource extends JsonResource
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
            'title' => $this->title,
            'isAllday' => (bool) $this->is_allday,
            'start' => $this->start->toISOString(),
            'end' => $this->end->toISOString(),
            'category' => $this->category,
            'location' => $this->location,
            'description' => $this->description,
            'color' => $this->color,
            'backgroundColor' => $this->bg_color,
            'borderColor' => $this->border_color,
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}
