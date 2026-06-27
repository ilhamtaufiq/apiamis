<?php

namespace App\Http\Controllers;

use App\Http\Requests\StoreBlogRequest;
use App\Http\Requests\UpdateBlogRequest;
use App\Http\Resources\BlogResource;
use App\Models\Blog;
use App\Models\BlogAsset;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class BlogController extends Controller
{
    public function index(Request $request): JsonResponse
    {
        $query = Blog::with('user');

        if (auth('sanctum')->check()) {
            $query->withCount('comments');
        }

        if (!auth('sanctum')->check()) {
            $query->where('is_published', true)
                  ->where('is_internal', false);
        } else {
            if ($request->has('published')) {
                $query->where('is_published', $request->boolean('published'));
            }
        }

        if ($request->has('category')) {
            $query->where('category', $request->category);
        }

        if ($request->boolean('featured')) {
            $query->where('is_featured', true);
        }

        $blogs = $query->latest()->paginate(15);

        return response()->json([
            'data' => BlogResource::collection($blogs),
            'meta' => [
                'current_page' => $blogs->currentPage(),
                'last_page' => $blogs->lastPage(),
                'per_page' => $blogs->perPage(),
                'total' => $blogs->total(),
            ],
        ]);
    }

    public function store(StoreBlogRequest $request): JsonResponse
    {
        $validated = $request->validated();
        $validated['user_id'] = auth()->id();

        if ($request->boolean('is_published')) {
            $validated['published_at'] = now();
        }

        $blog = Blog::create($validated);
        $this->attachReferencedVideoAssets($blog);

        return response()->json([
            'data' => new BlogResource($blog),
            'message' => 'Artikel berhasil dibuat',
        ], 201);
    }

    public function show($id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();

        if ($blog->is_internal && !auth('sanctum')->check()) {
            return response()->json([
                'message' => 'Postingan ini hanya untuk internal.',
            ], 403);
        }

        return response()->json([
            'data' => new BlogResource($blog),
        ]);
    }

    public function update(UpdateBlogRequest $request, $id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();
        $validated = $request->validated();

        if ($request->has('is_published')) {
            if ($request->boolean('is_published') && !$blog->is_published) {
                $validated['published_at'] = now();
            }
        }

        $blog->update($validated);
        $this->attachReferencedVideoAssets($blog);

        return response()->json([
            'data' => new BlogResource($blog),
            'message' => 'Artikel berhasil diperbarui',
        ]);
    }

    public function uploadVideo(Request $request): JsonResponse
    {
        $request->validate([
            'file' => 'required|file|mimetypes:video/mp4,video/webm,video/quicktime|max:102400',
            'poster' => 'nullable|image|mimes:jpeg,jpg,png,webp|max:5120',
        ]);

        $asset = BlogAsset::create([
            'user_id' => auth()->id(),
        ]);

        $media = $asset->addMediaFromRequest('file')
            ->toMediaCollection('blog/videos');

        $posterUrl = null;

        if ($request->hasFile('poster')) {
            $posterMedia = $asset->addMediaFromRequest('poster')
                ->toMediaCollection('blog/video-posters');

            $posterUrl = $posterMedia->getFullUrl();
        }

        return response()->json([
            'url' => $media->getFullUrl(),
            'media_id' => $media->id,
            'poster_url' => $posterUrl,
            'message' => 'Video berhasil diunggah',
        ]);
    }

    private function attachReferencedVideoAssets(Blog $blog): void
    {
        preg_match_all('/<video[^>]+src=["\']([^"\']+)["\']/i', $blog->content, $matches);
        $videoUrls = $matches[1] ?? [];

        if (empty($videoUrls)) {
            return;
        }

        $assets = BlogAsset::whereNull('blog_id')->get();

        foreach ($assets as $asset) {
            $media = $asset->getFirstMedia('blog/videos');

            if ($media && in_array($media->getFullUrl(), $videoUrls, true)) {
                $asset->update([
                    'blog_id' => $blog->id,
                ]);
            }
        }
    }

    public function feature($id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();

        if (!$blog->is_published || $blog->is_internal) {
            return response()->json([
                'message' => 'Hanya publikasi yang sudah terbit dan bersifat publik yang dapat dijadikan artikel utama.',
            ], 422);
        }

        Blog::where('is_featured', true)->update([
            'is_featured' => false,
            'featured_at' => null,
        ]);

        $blog->update([
            'is_featured' => true,
            'featured_at' => now(),
        ]);

        return response()->json([
            'data' => new BlogResource($blog->fresh('user')),
            'message' => 'Artikel utama berhasil diperbarui',
        ]);
    }

    public function unfeature($id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();

        $blog->update([
            'is_featured' => false,
            'featured_at' => null,
        ]);

        return response()->json([
            'data' => new BlogResource($blog->fresh('user')),
            'message' => 'Artikel tidak lagi menjadi artikel utama',
        ]);
    }

    public function destroy($id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();
        $blog->delete();

        return response()->json([
            'message' => 'Artikel berhasil dihapus',
        ]);
    }
}
