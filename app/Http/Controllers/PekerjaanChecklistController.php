<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\ChecklistItem;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class PekerjaanChecklistController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/pekerjaan-checklist",
     *     summary="List all pekerjaan with checklist status matrix",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="kegiatan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        // Get all checklist items (columns)
        $checklistItems = ChecklistItem::orderBy('sort_order')->get();

        // Build query for pekerjaan
        $query = Pekerjaan::with(['kegiatan'])
            ->byUserRole();

        // Filter by tahun
        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        // Filter by kegiatan
        if ($request->has('kegiatan_id') && $request->kegiatan_id) {
            $query->where('kegiatan_id', $request->kegiatan_id);
        }

        // Search
        if ($request->has('search') && $request->search) {
            $query->where('nama_paket', 'LIKE', '%' . $request->search . '%');
        }

        $pekerjaan = $query->orderBy('id')->paginate($request->per_page ?? 15);

        // Get all checklist data for these pekerjaan
        $pekerjaanIds = collect($pekerjaan->items())->pluck('id')->toArray();
        $checklistData = DB::table('pekerjaan_checklist')
            ->whereIn('pekerjaan_id', $pekerjaanIds)
            ->get()
            ->groupBy('pekerjaan_id');

        // Format response data
        $formattedData = collect($pekerjaan->items())->map(function($p) use ($checklistItems, $checklistData) {
            $checklist = [];
            foreach ($checklistItems as $item) {
                $data = $checklistData->get($p->id)?->firstWhere('checklist_item_id', $item->id);
                $checklist[$item->id] = [
                    'is_checked' => $data ? (bool) $data->is_checked : false,
                    'checked_at' => $data?->checked_at,
                    'checked_by' => $data?->checked_by,
                    'notes' => $data?->notes,
                ];
            }

            return [
                'id' => $p->id,
                'nama_paket' => $p->nama_paket,
                'kegiatan' => $p->kegiatan ? [
                    'id' => $p->kegiatan->id,
                    'nama_sub_kegiatan' => $p->kegiatan->nama_sub_kegiatan,
                ] : null,
                'checklist' => $checklist,
            ];
        });

        return response()->json([
            'columns' => $checklistItems->map(fn($i) => [
                'id' => $i->id,
                'name' => $i->name,
                'description' => $i->description,
                'sort_order' => $i->sort_order,
            ]),
            'data' => $formattedData,
            'meta' => [
                'current_page' => $pekerjaan->currentPage(),
                'last_page' => $pekerjaan->lastPage(),
                'per_page' => $pekerjaan->perPage(),
                'total' => $pekerjaan->total(),
            ]
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/pekerjaan-checklist/toggle",
     *     summary="Toggle or update a checklist item for a pekerjaan",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"pekerjaan_id", "checklist_item_id", "is_checked"},
     *             @OA\Property(property="pekerjaan_id", type="integer"),
     *             @OA\Property(property="checklist_item_id", type="integer"),
     *             @OA\Property(property="is_checked", type="boolean"),
     *             @OA\Property(property="notes", type="string")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function toggle(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'checklist_item_id' => 'required|exists:tbl_checklist_items,id',
            'is_checked' => 'required|boolean',
            'notes' => 'nullable|string',
        ]);

        $user = auth()->user();

        // Upsert the checklist record
        DB::table('pekerjaan_checklist')->updateOrInsert(
            [
                'pekerjaan_id' => $validated['pekerjaan_id'],
                'checklist_item_id' => $validated['checklist_item_id'],
            ],
            [
                'is_checked' => $validated['is_checked'],
                'checked_at' => $validated['is_checked'] ? now() : null,
                'checked_by' => $validated['is_checked'] ? $user?->id : null,
                'notes' => $validated['notes'] ?? null,
                'updated_at' => now(),
            ]
        );

        return response()->json([
            'message' => 'Checklist updated',
            'is_checked' => $validated['is_checked'],
        ]);
    }
}
