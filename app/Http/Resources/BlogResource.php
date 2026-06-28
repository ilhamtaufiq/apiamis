<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class BlogResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'title' => $this->title,
            'slug' => $this->slug,
            'content' => $this->content,
            'category' => $this->category,
            'cover_image' => $this->cover_image,
            'is_published' => $this->is_published,
            'is_internal' => $this->is_internal,
            'is_featured' => $this->is_featured,
            'published_at' => $this->published_at?->toIso8601String(),
            'featured_at' => $this->featured_at?->toIso8601String(),
            'user' => [
                'id' => $this->user_id,
                'name' => $this->user?->name,
                'avatar' => $this->user?->avatar,
                'gender' => $this->user?->gender,
                'jabatan' => $this->user?->jabatan,
            ],
            'comments_count' => $this->when(
                auth('sanctum')->check(),
                fn () => (int) ($this->comments_count ?? $this->comments()->count()),
            ),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
