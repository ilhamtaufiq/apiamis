<?php

namespace App\Http\Controllers;

use App\Models\ChecklistItem;
use App\Http\Resources\ChecklistItemResource;
use Illuminate\Http\Request;

class ChecklistItemController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/checklist-items",
     *     summary="List all checklist items",
     *     tags={"Checklist Management"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $items = ChecklistItem::orderBy('sort_order')->get();
        return ChecklistItemResource::collection($items);
    }

    /**
     * @OA\Post(
     *     path="/api/checklist-items",
     *     summary="Create new checklist item",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"name"},
     *             @OA\Property(property="name", type="string"),
     *             @OA\Property(property="description", type="string")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'name' => 'required|string|max:100',
            'description' => 'nullable|string|max:255',
        ]);

        // Auto-set sort order to last position
        $maxOrder = ChecklistItem::max('sort_order') ?? 0;
        $validated['sort_order'] = $maxOrder + 1;

        $item = ChecklistItem::create($validated);

        return new ChecklistItemResource($item);
    }

    /**
     * @OA\Get(
     *     path="/api/checklist-items/{id}",
     *     summary="Get checklist item detail",
     *     tags={"Checklist Management"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(ChecklistItem $checklistItem)
    {
        return new ChecklistItemResource($checklistItem);
    }

    /**
     * @OA\Put(
     *     path="/api/checklist-items/{id}",
     *     summary="Update checklist item",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, ChecklistItem $checklistItem)
    {
        $validated = $request->validate([
            'name' => 'sometimes|string|max:100',
            'description' => 'nullable|string|max:255',
            'sort_order' => 'sometimes|integer|min:0',
        ]);

        $checklistItem->update($validated);

        return new ChecklistItemResource($checklistItem);
    }

    /**
     * @OA\Delete(
     *     path="/api/checklist-items/{id}",
     *     summary="Delete checklist item",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(ChecklistItem $checklistItem)
    {
        $checklistItem->delete();

        return response()->json(['message' => 'Checklist item deleted successfully'], 200);
    }

    /**
     * @OA\Post(
     *     path="/api/checklist-items/reorder",
     *     summary="Reorder checklist items",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             @OA\Property(property="items", type="array", @OA\Items(
     *                 @OA\Property(property="id", type="integer"),
     *                 @OA\Property(property="sort_order", type="integer")
     *             ))
     *         )
     *     ),
     *     @OA\Response(response=200, description="Reorder success")
     * )
     */
    public function reorder(Request $request)
    {
        $validated = $request->validate([
            'items' => 'required|array',
            'items.*.id' => 'required|exists:tbl_checklist_items,id',
            'items.*.sort_order' => 'required|integer|min:0',
        ]);

        foreach ($validated['items'] as $item) {
            ChecklistItem::where('id', $item['id'])->update(['sort_order' => $item['sort_order']]);
        }

        return response()->json(['message' => 'Reorder successful'], 200);
    }
}
