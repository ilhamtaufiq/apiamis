<?php

namespace App\Http\Controllers;

use App\Models\Tag;
use App\Http\Resources\TagResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class TagController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/tags",
     *     summary="List all tags",
     *     tags={"Metadata (Tags)"},
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
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
     * @OA\Post(
     *     path="/api/tags",
     *     summary="Create new tag",
     *     tags={"Metadata (Tags)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"name"},
     *             @OA\Property(property="name", type="string"),
     *             @OA\Property(property="color", type="string", description="HEX color code e.g. #FF5733")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Created")
     * )
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
     * @OA\Get(
     *     path="/api/tags/{id}",
     *     summary="Get tag detail",
     *     tags={"Metadata (Tags)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Tag $tag)
    {
        return new TagResource($tag);
    }

    /**
     * @OA\Put(
     *     path="/api/tags/{id}",
     *     summary="Update tag",
     *     tags={"Metadata (Tags)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
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
     * @OA\Delete(
     *     path="/api/tags/{id}",
     *     summary="Delete tag",
     *     tags={"Metadata (Tags)"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Tag $tag)
    {
        $tag->delete();

        return response()->json(['message' => 'Tag deleted successfully'], 200);
    }
}
