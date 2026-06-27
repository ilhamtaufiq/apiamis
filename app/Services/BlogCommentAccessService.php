<?php

namespace App\Services;

use App\Models\Blog;
use App\Models\BlogComment;
use App\Models\User;

class BlogCommentAccessService
{
    public function canViewBlogComments(Blog $blog): bool
    {
        if ($blog->is_internal && ! auth('sanctum')->check()) {
            return false;
        }

        return true;
    }

    public function canCommentOnBlog(Blog $blog): bool
    {
        if (! auth('sanctum')->check()) {
            return false;
        }

        return (bool) $blog->is_published;
    }

    public function canDeleteComment(BlogComment $comment, ?User $user): bool
    {
        if (! $user) {
            return false;
        }

        if ($comment->user_id === $user->id) {
            return true;
        }

        return $user->hasRole('admin');
    }

    public function canEditComment(BlogComment $comment, ?User $user): bool
    {
        if (! $user || $comment->trashed()) {
            return false;
        }

        return $comment->user_id === $user->id;
    }
}