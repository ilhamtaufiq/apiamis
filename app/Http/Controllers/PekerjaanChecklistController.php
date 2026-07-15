<?php

namespace App\Http\Controllers;

use App\Exports\PekerjaanChecklistExport;
use App\Models\ChecklistItem;
use App\Models\Pekerjaan;
use App\Models\PekerjaanChecklistHistory;
use App\Models\User;
use Dompdf\Dompdf;
use Dompdf\Options;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\View;
use Maatwebsite\Excel\Facades\Excel;

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
        $checklistItems = ChecklistItem::where('context', 'pekerjaan')->orderBy('sort_order')->get();

        $query = Pekerjaan::with(['kegiatan'])->byUserRole();

        if ($request->filled('tahun')) {
            $query->whereHas('kegiatan', function ($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->filled('kegiatan_id')) {
            $query->where('kegiatan_id', $request->kegiatan_id);
        }

        if ($request->filled('search')) {
            $query->where('nama_paket', 'LIKE', '%'.$request->search.'%');
        }

        $pekerjaan = $query->orderBy('id')->paginate($request->per_page ?? 15);

        $pekerjaanIds = collect($pekerjaan->items())->pluck('id')->toArray();
        $checklistData = DB::table('pekerjaan_checklist')
            ->whereIn('pekerjaan_id', $pekerjaanIds)
            ->get()
            ->groupBy('pekerjaan_id');

        $userIds = $checklistData->flatten()->pluck('checked_by')->filter()->unique()->values();
        $users = User::whereIn('id', $userIds)->pluck('name', 'id');

        $formattedData = collect($pekerjaan->items())->map(function ($p) use ($checklistItems, $checklistData, $users) {
            $checklist = [];
            $latestUpdatedAt = null;
            $latestUpdatedBy = null;
            $latestUpdatedByName = null;

            foreach ($checklistItems as $item) {
                $data = $checklistData->get($p->id)?->firstWhere('checklist_item_id', $item->id);
                $updatedAt = $data?->updated_at ?? $data?->checked_at;
                $checkedBy = $data?->checked_by;

                $checklist[$item->id] = [
                    'is_checked' => $data ? (bool) $data->is_checked : false,
                    'checked_at' => $data?->checked_at,
                    'updated_at' => $updatedAt,
                    'checked_by' => $checkedBy,
                    'checked_by_name' => $checkedBy ? ($users[$checkedBy] ?? null) : null,
                    'notes' => $data?->notes,
                ];

                if ($updatedAt && ($latestUpdatedAt === null || $updatedAt > $latestUpdatedAt)) {
                    $latestUpdatedAt = $updatedAt;
                    $latestUpdatedBy = $checkedBy;
                    $latestUpdatedByName = $checkedBy ? ($users[$checkedBy] ?? null) : null;
                }
            }

            return [
                'id' => $p->id,
                'nama_paket' => $p->nama_paket,
                'kegiatan' => $p->kegiatan ? [
                    'id' => $p->kegiatan->id,
                    'nama_sub_kegiatan' => $p->kegiatan->nama_sub_kegiatan,
                ] : null,
                'checklist' => $checklist,
                'last_updated_at' => $latestUpdatedAt,
                'last_updated_by' => $latestUpdatedBy,
                'last_updated_by_name' => $latestUpdatedByName,
            ];
        });

        return response()->json([
            'columns' => $checklistItems->map(fn ($i) => [
                'id' => $i->id,
                'name' => $i->name,
                'description' => $i->description,
                'sort_order' => $i->sort_order,
            ]),
            'data' => $formattedData,
            'meta' => [
                'current_page' => $pekerjaan->currentPage(),
                'from' => $pekerjaan->firstItem(),
                'last_page' => $pekerjaan->lastPage(),
                'per_page' => $pekerjaan->perPage(),
                'to' => $pekerjaan->lastItem(),
                'total' => $pekerjaan->total(),
            ],
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/pekerjaan-checklist/toggle",
     *     summary="Toggle or update a checklist item for a pekerjaan",
     *     tags={"Checklist Management"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
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
        $now = now();

        $existing = DB::table('pekerjaan_checklist')
            ->where('pekerjaan_id', $validated['pekerjaan_id'])
            ->where('checklist_item_id', $validated['checklist_item_id'])
            ->first();

        $payload = [
            'is_checked' => $validated['is_checked'],
            // Keep checked_at as last time this item was marked complete
            'checked_at' => $validated['is_checked'] ? $now : ($existing->checked_at ?? null),
            // Always record last actor (check or uncheck)
            'checked_by' => $user?->id,
            'notes' => $validated['notes'] ?? null,
            'updated_at' => $now,
        ];

        if ($existing) {
            DB::table('pekerjaan_checklist')
                ->where('id', $existing->id)
                ->update($payload);
        } else {
            DB::table('pekerjaan_checklist')->insert(array_merge($payload, [
                'pekerjaan_id' => $validated['pekerjaan_id'],
                'checklist_item_id' => $validated['checklist_item_id'],
                'created_at' => $now,
            ]));
        }

        PekerjaanChecklistHistory::create([
            'pekerjaan_id' => $validated['pekerjaan_id'],
            'checklist_item_id' => $validated['checklist_item_id'],
            'is_checked' => $validated['is_checked'],
            'notes' => $validated['notes'] ?? null,
            'user_id' => $user?->id,
            'created_at' => $now,
        ]);

        return response()->json([
            'message' => 'Checklist updated',
            'is_checked' => $validated['is_checked'],
            'checked_by' => $user?->id,
            'checked_by_name' => $user?->name,
            'updated_at' => $now->toDateTimeString(),
        ]);
    }

    /**
     * History of checklist changes (who changed what and when).
     */
    public function history(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'checklist_item_id' => 'nullable|integer|exists:tbl_checklist_items,id',
            'user_id' => 'nullable|integer|exists:users,id',
            'tahun' => 'nullable',
            'search' => 'nullable|string',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:100',
        ]);

        $allowedPekerjaanIds = Pekerjaan::query()->byUserRole()->pluck('id');

        $query = PekerjaanChecklistHistory::query()
            ->with([
                'pekerjaan:id,nama_paket,kegiatan_id',
                'pekerjaan.kegiatan:id,nama_sub_kegiatan,tahun_anggaran',
                'checklistItem:id,name',
                'user:id,name,email',
            ])
            ->whereIn('pekerjaan_id', $allowedPekerjaanIds)
            ->orderByDesc('created_at')
            ->orderByDesc('id');

        if (! empty($validated['pekerjaan_id'])) {
            $query->where('pekerjaan_id', $validated['pekerjaan_id']);
        }

        if (! empty($validated['checklist_item_id'])) {
            $query->where('checklist_item_id', $validated['checklist_item_id']);
        }

        if (! empty($validated['user_id'])) {
            $query->where('user_id', $validated['user_id']);
        }

        if (! empty($validated['tahun'])) {
            $query->whereHas('pekerjaan.kegiatan', function ($q) use ($validated) {
                $q->where('tahun_anggaran', $validated['tahun']);
            });
        }

        if (! empty($validated['search'])) {
            $search = $validated['search'];
            $query->where(function ($q) use ($search) {
                $q->whereHas('pekerjaan', function ($pq) use ($search) {
                    $pq->where('nama_paket', 'LIKE', '%'.$search.'%');
                })->orWhereHas('checklistItem', function ($cq) use ($search) {
                    $cq->where('name', 'LIKE', '%'.$search.'%');
                })->orWhereHas('user', function ($uq) use ($search) {
                    $uq->where('name', 'LIKE', '%'.$search.'%');
                });
            });
        }

        $histories = $query->paginate($validated['per_page'] ?? 20);

        $data = collect($histories->items())->map(function (PekerjaanChecklistHistory $h) {
            return [
                'id' => $h->id,
                'pekerjaan_id' => $h->pekerjaan_id,
                'pekerjaan_nama' => $h->pekerjaan?->nama_paket,
                'kegiatan' => $h->pekerjaan?->kegiatan?->nama_sub_kegiatan,
                'checklist_item_id' => $h->checklist_item_id,
                'checklist_item_name' => $h->checklistItem?->name,
                'is_checked' => (bool) $h->is_checked,
                'notes' => $h->notes,
                'user_id' => $h->user_id,
                'user_name' => $h->user?->name,
                'user_email' => $h->user?->email,
                'created_at' => $h->created_at?->toDateTimeString(),
            ];
        });

        return response()->json([
            'data' => $data,
            'meta' => [
                'current_page' => $histories->currentPage(),
                'from' => $histories->firstItem(),
                'last_page' => $histories->lastPage(),
                'per_page' => $histories->perPage(),
                'to' => $histories->lastItem(),
                'total' => $histories->total(),
            ],
        ]);
    }

    public function exportExcel(Request $request)
    {
        $filename = 'checklist_pekerjaan_'.now()->format('Ymd_His').'.xlsx';

        return Excel::download(
            new PekerjaanChecklistExport(
                $request->tahun,
                $request->kegiatan_id,
                $request->search
            ),
            $filename
        );
    }

    public function exportPdf(Request $request)
    {
        $export = new PekerjaanChecklistExport(
            $request->tahun,
            $request->kegiatan_id,
            $request->search
        );

        $rows = $export->collection();
        $headings = $export->headings();
        $mapped = $rows->map(fn ($row) => $export->map($row))->values();

        $html = View::make('exports.pekerjaan-checklist-pdf', [
            'title' => 'Checklist Pekerjaan',
            'tahun' => $request->tahun,
            'generatedAt' => now()->format('d/m/Y H:i'),
            'headings' => $headings,
            'rows' => $mapped,
        ])->render();

        $options = new Options;
        $options->set('isRemoteEnabled', false);
        $options->set('defaultFont', 'DejaVu Sans');

        $dompdf = new Dompdf($options);
        $dompdf->loadHtml($html);
        $dompdf->setPaper('a4', 'landscape');
        $dompdf->render();

        $filename = 'checklist_pekerjaan_'.now()->format('Ymd_His').'.pdf';

        return response($dompdf->output(), 200, [
            'Content-Type' => 'application/pdf',
            'Content-Disposition' => 'attachment; filename="'.$filename.'"',
        ]);
    }
}
