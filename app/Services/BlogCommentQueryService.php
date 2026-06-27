<?php

namespace App\Services;

use App\Models\Blog;
use App\Models\BlogComment;
use Illuminate\Contracts\Pagination\LengthAwarePaginator;
use Illuminate\Database\Eloquent\Builder;
use Illuminate\Support\Collection;

class BlogCommentQueryService
{
    public const ROOT_PER_PAGE = 20;

    public function paginatedRoots(
        Blog $blog,
        int $page = 1,
        int $perPage = self::ROOT_PER_PAGE,
        string $sort = 'oldest',
    ): LengthAwarePaginator {
        $query = BlogComment::query()
            ->where('blog_id', $blog->id)
            ->whereNull('parent_id');

        if ($sort === 'newest') {
            $query->orderByDesc('created_at');
        } else {
            $query->orderBy('created_at');
        }

        return $query->paginate($perPage, ['*'], 'page', $page);
    }

    public function collectThreadComments(Blog $blog, array $rootIds): Collection
    {
        if ($rootIds === []) {
            return collect();
        }

        $comments = BlogComment::withTrashed()
            ->with('user')
            ->where('blog_id', $blog->id)
            ->whereIn('id', $rootIds)
            ->get();

        $frontier = $rootIds;

        while ($frontier !== []) {
            $children = BlogComment::withTrashed()
                ->with('user')
                ->where('blog_id', $blog->id)
                ->whereIn('parent_id', $frontier)
                ->get();

            if ($children->isEmpty()) {
                break;
            }

            $comments = $comments->merge($children);
            $frontier = $children->pluck('id')->all();
        }

        return $comments->sortBy('created_at')->values();
    }

    public function totalComments(Blog $blog): int
    {
        return BlogComment::where('blog_id', $blog->id)->count();
    }

    public function collectThreadForComment(Blog $blog, int $commentId): Collection
    {
        $comment = BlogComment::withTrashed()
            ->where('blog_id', $blog->id)
            ->where('id', $commentId)
            ->first();

        if (! $comment) {
            return collect();
        }

        $root = $comment;
        while ($root->parent_id) {
            $parent = BlogComment::withTrashed()
                ->where('blog_id', $blog->id)
                ->where('id', $root->parent_id)
                ->first();

            if (! $parent) {
                break;
            }

            $root = $parent;
        }

        return $this->collectThreadComments($blog, [$root->id]);
    }

    public function paginatedAdminComments(
        int $page = 1,
        int $perPage = 20,
        ?int $blogId = null,
        ?string $search = null,
        ?string $status = null,
    ): LengthAwarePaginator {
        $query = BlogComment::withTrashed()
            ->with(['user', 'blog'])
            ->orderByDesc('created_at');

        if ($blogId) {
            $query->where('blog_id', $blogId);
        }

        if ($search) {
            $query->where(function (Builder $builder) use ($search) {
                $builder->where('body', 'like', "%{$search}%")
                    ->orWhereHas('user', fn (Builder $userQuery) => $userQuery->where('name', 'like', "%{$search}%"))
                    ->orWhereHas('blog', fn (Builder $blogQuery) => $blogQuery->where('title', 'like', "%{$search}%"));
            });
        }

        if ($status === 'deleted') {
            $query->onlyTrashed();
        } elseif ($status === 'active') {
            $query->whereNull('deleted_at');
        }

        return $query->paginate($perPage, ['*'], 'page', $page);
    }
}