<?php

namespace App\Http\Resources;

use App\Services\BlogCommentAccessService;
use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class BlogCommentResource extends JsonResource
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
            'is_deleted' => $isDeleted,
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