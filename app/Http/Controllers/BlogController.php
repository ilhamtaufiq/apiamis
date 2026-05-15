<?php

namespace App\Http\Controllers;

use App\Http\Requests\StoreBlogRequest;
use App\Http\Requests\UpdateBlogRequest;
use App\Http\Resources\BlogResource;
use App\Models\Blog;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class BlogController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request): JsonResponse
    {
        $query = Blog::with('user');

        // Guest users can only see published and public (non-internal) posts
        if (!auth('sanctum')->check()) {
            $query->where('is_published', true)
                  ->where('is_internal', false);
        } else {
            // Authenticated users can see internal posts.
            // They can also filter by published status (useful for management)
            if ($request->has('published')) {
                $query->where('is_published', $request->boolean('published'));
            }
        }

        if ($request->has('category')) {
            $query->where('category', $request->category);
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

    /**
     * Store a newly created resource in storage.
     */
    public function store(StoreBlogRequest $request): JsonResponse
    {
        $validated = $request->validated();
        $validated['user_id'] = auth()->id();
        
        if ($request->boolean('is_published')) {
            $validated['published_at'] = now();
        }

        $blog = Blog::create($validated);

        return response()->json([
            'data' => new BlogResource($blog),
            'message' => 'Artikel berhasil dibuat',
        ], 201);
    }

    /**
     * Display the specified resource.
     */
    public function show($id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();
        
        // If post is internal and user is not logged in, forbid access
        if ($blog->is_internal && !auth('sanctum')->check()) {
            return response()->json([
                'message' => 'Postingan ini hanya untuk internal.',
            ], 403);
        }

        return response()->json([
            'data' => new BlogResource($blog),
        ]);
    }

    /**
     * Update the specified resource in storage.
     */
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

        return response()->json([
            'data' => new BlogResource($blog),
            'message' => 'Artikel berhasil diperbarui',
        ]);
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy($id): JsonResponse
    {
        $blog = Blog::where('id', $id)->orWhere('slug', $id)->firstOrFail();
        $blog->delete();

        return response()->json([
            'message' => 'Artikel berhasil dihapus',
        ]);
    }
}
