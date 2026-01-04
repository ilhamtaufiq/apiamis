<?php

namespace App\Http\Controllers;

use App\Models\ChecklistItem;
use App\Http\Resources\ChecklistItemResource;
use Illuminate\Http\Request;

class ChecklistItemController extends Controller
{
    /**
     * Display a listing of checklist items.
     */
    public function index()
    {
        $items = ChecklistItem::orderBy('sort_order')->get();
        return ChecklistItemResource::collection($items);
    }

    /**
     * Store a newly created checklist item.
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
     * Display the specified checklist item.
     */
    public function show(ChecklistItem $checklistItem)
    {
        return new ChecklistItemResource($checklistItem);
    }

    /**
     * Update the specified checklist item.
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
     * Remove the specified checklist item.
     */
    public function destroy(ChecklistItem $checklistItem)
    {
        $checklistItem->delete();

        return response()->json(['message' => 'Checklist item deleted successfully'], 200);
    }

    /**
     * Reorder checklist items.
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
