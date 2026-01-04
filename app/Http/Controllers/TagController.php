<?php

namespace App\Http\Controllers;

use App\Models\Tag;
use App\Http\Resources\TagResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class TagController extends Controller
{
    /**
     * Display a listing of tags.
     */
    public function index(Request $request)
    {
        $query = Tag::query();

        // Search functionality
        if ($request->has('search') && !empty($request->search)) {
            $query->where('name', 'LIKE', '%' . $request->search . '%');
        }

        // Return all tags (usually small dataset)
        $tags = $query->orderBy('name')->get();

        return TagResource::collection($tags);
    }

    /**
     * Store a newly created tag.
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'name' => 'required|string|max:100|unique:tbl_tags,name',
            'color' => 'nullable|string|max:7|regex:/^#[0-9A-Fa-f]{6}$/',
        ]);

        $validated['slug'] = Str::slug($validated['name']);

        $tag = Tag::create($validated);

        return new TagResource($tag);
    }

    /**
     * Display the specified tag.
     */
    public function show(Tag $tag)
    {
        return new TagResource($tag);
    }

    /**
     * Update the specified tag.
     */
    public function update(Request $request, Tag $tag)
    {
        $validated = $request->validate([
            'name' => 'sometimes|string|max:100|unique:tbl_tags,name,' . $tag->id,
            'color' => 'nullable|string|max:7|regex:/^#[0-9A-Fa-f]{6}$/',
        ]);

        if (isset($validated['name'])) {
            $validated['slug'] = Str::slug($validated['name']);
        }

        $tag->update($validated);

        return new TagResource($tag);
    }

    /**
     * Remove the specified tag.
     */
    public function destroy(Tag $tag)
    {
        $tag->delete();

        return response()->json(['message' => 'Tag deleted successfully'], 200);
    }
}
