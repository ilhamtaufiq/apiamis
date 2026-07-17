<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

/** @mixin \App\Models\PanduanPage */
class PanduanPageResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'slug' => $this->slug,
            'title' => $this->title,
            'description' => $this->description,
            'section' => $this->section,
            'sort_order' => $this->sort_order,
            'body' => $this->body,
            'is_published' => $this->is_published,
            'updated_by' => $this->updated_by,
            'editor' => $this->whenLoaded('editor', fn () => $this->editor ? [
                'id' => $this->editor->id,
                'name' => $this->editor->name,
            ] : null),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
