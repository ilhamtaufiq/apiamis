<?php

namespace App\Http\Controllers;

use App\Http\Resources\BlogCommentAdminResource;
use App\Http\Resources\BlogCommentResource;
use App\Models\Blog;
use App\Models\BlogComment;
use App\Notifications\AppNotification;
use App\Services\BlogCommentAccessService;
use App\Services\BlogCommentQueryService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Validator;
use Illuminate\Support\Str;

class BlogCommentController extends Controller
{
    public function __construct(
        private readonly BlogCommentAccessService $access,
        private readonly BlogCommentQueryService $comments,
    ) {}

    public function adminIndex(Request $request): JsonResponse
    {
        if (! auth('sanctum')->check()) {
            return response()->json([
                'message' => 'Anda harus login untuk mengakses daftar komentar.',
            ], 401);
        }

        $perPage = min(max((int) $request->integer('per_page', 20), 1), 50);
        $page = max((int) $request->integer('page', 1), 1);
        $blogId = $request->filled('blog_id') ? (int) $request->integer('blog_id') : null;
        $search = $request->filled('search') ? $request->string('search')->toString() : null;
        $status = $request->string('status')->toString();
        $status = in_array($status, ['active', 'deleted', 'all'], true) ? $status : 'all';

        $comments = $this->comments->paginatedAdminComments(
            $page,
            $perPage,
            $blogId,
            $search,
            $status === 'all' ? null : $status,
        );

        return response()->json([
            'data' => BlogCommentAdminResource::collection($comments),
            'meta' => [
                'current_page' => $comments->currentPage(),
                'last_page' => $comments->lastPage(),
                'per_page' => $comments->perPage(),
                'total' => $comments->total(),
            ],
        ]);
    }

    public function index(Request $request, Blog $blog): JsonResponse
    {
        if (! $this->access->canViewBlogComments($blog)) {
            return response()->json([
                'message' => 'Komentar hanya dapat diakses oleh pengguna yang login.',
            ], 403);
        }

        $perPage = min(max((int) $request->integer('per_page', BlogCommentQueryService::ROOT_PER_PAGE), 1), 50);
        $page = max((int) $request->integer('page', 1), 1);
        $sort = $request->string('sort')->toString() === 'newest' ? 'newest' : 'oldest';

        $roots = $this->comments->paginatedRoots($blog, $page, $perPage, $sort);
        $rootIds = $roots->getCollection()->pluck('id')->all();
        $threadComments = $this->comments->collectThreadComments($blog, $rootIds);

        return response()->json([
            'data' => BlogCommentResource::collection($threadComments),
            'meta' => [
                'total' => $this->comments->totalComments($blog),
                'root_total' => $roots->total(),
                'current_page' => $roots->currentPage(),
                'last_page' => $roots->lastPage(),
                'per_page' => $roots->perPage(),
                'sort' => $sort,
            ],
        ]);
    }

    public function thread(Blog $blog, BlogComment $comment): JsonResponse
    {
        if (! $this->access->canViewBlogComments($blog)) {
            return response()->json([
                'message' => 'Komentar hanya dapat diakses oleh pengguna yang login.',
            ], 403);
        }

        if ($comment->blog_id !== $blog->id) {
            return response()->json([
                'message' => 'Komentar tidak ditemukan.',
            ], 404);
        }

        $threadComments = $this->comments->collectThreadForComment($blog, $comment->id);

        return response()->json([
            'data' => BlogCommentResource::collection($threadComments),
        ]);
    }

    public function count(Blog $blog): JsonResponse
    {
        if (! $this->access->canViewBlogComments($blog)) {
            return response()->json([
                'message' => 'Komentar hanya dapat diakses oleh pengguna yang login.',
            ], 403);
        }

        return response()->json([
            'total' => $this->comments->totalComments($blog),
        ]);
    }

    public function store(Request $request, Blog $blog): JsonResponse
    {
        if (! $this->access->canCommentOnBlog($blog)) {
            return response()->json([
                'message' => auth('sanctum')->check()
                    ? 'Komentar hanya tersedia untuk artikel yang sudah terbit.'
                    : 'Anda harus login untuk berkomentar.',
            ], auth('sanctum')->check() ? 422 : 401);
        }

        if ($blog->is_internal && ! auth('sanctum')->check()) {
            return response()->json([
                'message' => 'Artikel internal hanya dapat dikomentari oleh pengguna login.',
            ], 403);
        }

        $validator = Validator::make($request->all(), [
            'body' => 'required|string|max:5000',
            'parent_id' => 'nullable|integer|exists:tbl_blog_comment,id',
        ]);

        if ($validator->fails()) {
            return response()->json([
                'message' => 'Validasi gagal',
                'errors' => $validator->errors(),
            ], 422);
        }

        $body = $this->sanitizeBody($request->input('body'));
        if ($body === '') {
            return response()->json([
                'message' => 'Isi komentar tidak boleh kosong.',
            ], 422);
        }

        if ($this->isDuplicateSpam($blog, $body)) {
            return response()->json([
                'message' => 'Komentar identik baru saja dikirim. Tunggu sebentar sebelum mengirim ulang.',
            ], 429);
        }

        $depth = 0;
        $parentId = $request->input('parent_id');
        $parent = null;

        if ($parentId) {
            $parent = BlogComment::where('blog_id', $blog->id)
                ->where('id', $parentId)
                ->first();

            if (! $parent) {
                return response()->json([
                    'message' => 'Komentar induk tidak ditemukan.',
                ], 422);
            }

            if ($parent->depth >= BlogComment::MAX_DEPTH) {
                return response()->json([
                    'message' => 'Balasan terlalu dalam (maksimal ' . BlogComment::MAX_DEPTH . ' level).',
                ], 422);
            }

            $depth = $parent->depth + 1;
        }

        $comment = BlogComment::create([
            'blog_id' => $blog->id,
            'user_id' => auth()->id(),
            'parent_id' => $parentId,
            'body' => $body,
            'depth' => $depth,
        ]);

        $comment->load('user');
        $this->notifyParticipants($blog, $comment, $parent);

        return response()->json([
            'data' => new BlogCommentResource($comment),
            'message' => 'Komentar berhasil dikirim',
        ], 201);
    }

    public function update(Request $request, BlogComment $comment): JsonResponse
    {
        $user = auth()->user();

        if (! $this->access->canEditComment($comment, $user)) {
            return response()->json([
                'message' => 'Anda tidak memiliki izin mengedit komentar ini.',
            ], 403);
        }

        $validator = Validator::make($request->all(), [
            'body' => 'required|string|max:5000',
        ]);

        if ($validator->fails()) {
            return response()->json([
                'message' => 'Validasi gagal',
                'errors' => $validator->errors(),
            ], 422);
        }

        $body = $this->sanitizeBody($request->input('body'));
        if ($body === '') {
            return response()->json([
                'message' => 'Isi komentar tidak boleh kosong.',
            ], 422);
        }

        if ($body === $comment->body) {
            return response()->json([
                'data' => new BlogCommentResource($comment->loadMissing('user')),
                'message' => 'Tidak ada perubahan pada komentar.',
            ]);
        }

        $comment->update(['body' => $body]);
        $comment->load('user');

        return response()->json([
            'data' => new BlogCommentResource($comment),
            'message' => 'Komentar berhasil diperbarui',
        ]);
    }

    public function destroy(BlogComment $comment): JsonResponse
    {
        $user = auth()->user();

        if (! $this->access->canDeleteComment($comment, $user)) {
            return response()->json([
                'message' => 'Anda tidak memiliki izin menghapus komentar ini.',
            ], 403);
        }

        $comment->delete();

        return response()->json([
            'message' => 'Komentar berhasil dihapus',
        ]);
    }

    private function isDuplicateSpam(Blog $blog, string $body): bool
    {
        return BlogComment::query()
            ->where('blog_id', $blog->id)
            ->where('user_id', auth()->id())
            ->where('body', $body)
            ->where('created_at', '>=', now()->subSeconds(30))
            ->exists();
    }

    private function notifyParticipants(Blog $blog, BlogComment $comment, ?BlogComment $parent): void
    {
        $actorName = auth()->user()->name;
        $articleUrl = '/publikasi/' . $blog->slug . '#comment-' . $comment->id;

        if ($parent) {
            $parent->loadMissing('user');
            $parentAuthor = $parent->user;

            if ($parentAuthor && $parentAuthor->id !== auth()->id()) {
                $parentAuthor->notify(new AppNotification(
                    'Balasan komentar baru',
                    $actorName . ' membalas komentar Anda di "' . $blog->title . '".',
                    $articleUrl,
                    'info',
                ));
            }
        }

        $blog->loadMissing('user');
        $postAuthor = $blog->user;

        if ($postAuthor && $postAuthor->id !== auth()->id()) {
            if (! $parent || $parent->user_id !== $postAuthor->id) {
                $postAuthor->notify(new AppNotification(
                    'Komentar baru pada publikasi',
                    $actorName . ' berkomentar di "' . $blog->title . '".',
                    $articleUrl,
                    'info',
                ));
            }
        }
    }

    private function sanitizeBody(string $body): string
    {
        return Str::of($body)
            ->replaceMatches('/<[^>]*>/', '')
            ->trim()
            ->toString();
    }
}