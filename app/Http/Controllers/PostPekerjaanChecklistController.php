<?php

namespace App\Http\Controllers;

use App\Models\ChecklistItem;
use App\Models\Pekerjaan;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class PostPekerjaanChecklistController extends Controller
{
    public function index(Request $request)
    {
        $checklistItems = ChecklistItem::query()
            ->where('context', 'post_pekerjaan')
            ->orderBy('sort_order')
            ->get();

        $query = Pekerjaan::with(['kegiatan', 'kontrak.penyedia'])
            ->byUserRole()
            ->whereHas('kontrak');

        if ($request->filled('tahun')) {
            $query->whereHas('kegiatan', function ($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->filled('kegiatan_id')) {
            $query->where('kegiatan_id', $request->kegiatan_id);
        }

        if ($request->filled('search')) {
            $query->where(function ($q) use ($request) {
                $search = $request->search;
                $q->where('nama_paket', 'LIKE', "%{$search}%")
                    ->orWhereHas('kontrak', function ($kontrakQuery) use ($search) {
                        $kontrakQuery->where('nomor_penawaran', 'LIKE', "%{$search}%")
                            ->orWhere('spk', 'LIKE', "%{$search}%")
                            ->orWhere('kode_paket', 'LIKE', "%{$search}%");
                    });
            });
        }

        $pekerjaan = $query->orderBy('id')->paginate($request->per_page ?? 15);
        $pekerjaanIds = collect($pekerjaan->items())->pluck('id')->toArray();

        $checklistData = DB::table('pekerjaan_checklist')
            ->whereIn('pekerjaan_id', $pekerjaanIds)
            ->get()
            ->groupBy('pekerjaan_id');

        $formattedData = collect($pekerjaan->items())->map(function ($p) use ($checklistItems, $checklistData) {
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

            $kontrak = $p->kontrak->first();

            return [
                'id' => $p->id,
                'nama_paket' => $p->nama_paket,
                'kegiatan' => $p->kegiatan ? [
                    'id' => $p->kegiatan->id,
                    'nama_sub_kegiatan' => $p->kegiatan->nama_sub_kegiatan,
                ] : null,
                'kontrak' => $kontrak ? [
                    'id' => $kontrak->id,
                    'nomor_penawaran' => $kontrak->nomor_penawaran,
                    'spk' => $kontrak->spk,
                    'kode_paket' => $kontrak->kode_paket,
                    'penyedia' => $kontrak->penyedia?->nama,
                ] : null,
                'checklist' => $checklist,
            ];
        });

        return response()->json([
            'columns' => $checklistItems->map(fn ($i) => [
                'id' => $i->id,
                'name' => $i->name,
                'description' => $i->description,
                'sort_order' => $i->sort_order,
                'context' => $i->context,
            ]),
            'data' => $formattedData,
            'meta' => [
                'current_page' => $pekerjaan->currentPage(),
                'last_page' => $pekerjaan->lastPage(),
                'per_page' => $pekerjaan->perPage(),
                'total' => $pekerjaan->total(),
            ],
        ]);
    }
}