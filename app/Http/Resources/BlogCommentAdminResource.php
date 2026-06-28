<?php

namespace App\Http\Resources;

use App\Services\BlogCommentAccessService;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Str;

class BlogCommentAdminResource extends JsonResource
{
    public static $wrap = null;

    public function toArray(Request $request): array
    {
        $access = app(BlogCommentAccessService::class);
        $user = $request->user();
        $isDeleted = $this->trashed();

        return [
            'id' => $this->id,
            'blog_id' => $this->blog_id,
            'parent_id' => $this->parent_id,
            'depth' => $this->depth,
            'body' => $isDeleted ? null : $this->body,
            'body_preview' => $isDeleted
                ? null
                : Str::limit(preg_replace('/\s+/', ' ', $this->body ?? '') ?? '', 160),
            'is_deleted' => $isDeleted,
            'blog' => [
                'id' => $this->blog?->id,
                'title' => $this->blog?->title,
                'slug' => $this->blog?->slug,
                'is_published' => (bool) $this->blog?->is_published,
            ],
            'user' => $isDeleted ? null : [
                'id' => $this->user?->id,
                'name' => $this->user?->name,
                'avatar' => $this->user?->avatar,
                'gender' => $this->user?->gender,
                'jabatan' => $this->user?->jabatan,
            ],
            'can_delete' => $access->canDeleteComment($this->resource, $user),
            'can_edit' => $access->canEditComment($this->resource, $user),
            'is_edited' => ! $isDeleted && $this->updated_at && $this->created_at
                && $this->updated_at->gt($this->created_at->copy()->addSeconds(2)),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}