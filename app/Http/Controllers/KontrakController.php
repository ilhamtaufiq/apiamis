<?php

namespace App\Http\Controllers;

use App\Exports\KontrakExport;
use App\Exports\KontrakTemplateExport;
use App\Http\Resources\KontrakDetailResource;
use App\Http\Resources\KontrakResource;
use App\Imports\KontrakImport;
use App\Models\Kontrak;
use Illuminate\Http\Request;
use Illuminate\Support\Str;
use Maatwebsite\Excel\Facades\Excel;

class KontrakController extends Controller
{
    protected $baService;

    protected $exportService;

    public function __construct(\App\Services\DocumentExportService $exportService)
    {
        $this->exportService = $exportService;
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak",
     *     summary="List all kontrak",
     *     tags={"Kontrak"},
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Kontrak::with(['kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.kegiatan', 'penyedia'])
            ->orderBy('tgl_spk', 'desc')
            ->orderBy('created_at', 'desc');

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('kegiatan', function ($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        // Search functionality
        if ($request->has('search') && $request->search) {
            $search = $request->search;
            $query->where(function ($q) use ($search) {
                $q->where('kode_rup', 'like', "%{$search}%")
                    ->orWhere('nomor_penawaran', 'like', "%{$search}%")
                    ->orWhere('kode_paket', 'like', "%{$search}%")
                    ->orWhereHas('pekerjaans', function ($q) use ($search) {
                        $q->where('nama_paket', 'like', "%{$search}%");
                    })
                    ->orWhereHas('penyedia', function ($q) use ($search) {
                        $q->where('nama', 'like', "%{$search}%");
                    });
            });
        }

        $kontrak = $query->paginate(20);

        return KontrakResource::collection($kontrak);
    }

    /**
     * @OA\Post(
     *     path="/api/kontrak",
     *     summary="Create new kontrak",
     *     tags={"Kontrak"},
     *
     *     @OA\RequestBody(
     *         required=true,
     *
     *         @OA\JsonContent(
     *             required={"id_pekerjaan", "id_penyedia"},
     *
     *             @OA\Property(property="id_pekerjaan", type="integer"),
     *             @OA\Property(property="id_penyedia", type="integer")
     *         )
     *     ),
     *
     *     @OA\Response(response=201, description="Kontrak created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'id_kegiatan' => 'nullable|integer|exists:tbl_kegiatan,id',
            'pekerjaan_ids' => 'nullable|array',
            'pekerjaan_ids.*' => 'integer|exists:tbl_pekerjaan,id',
            'id_pekerjaan' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'id_penyedia' => 'required|integer|exists:tbl_penyedia,id',
            'kode_rup' => 'nullable|string|max:50',
            'kode_paket' => 'nullable|string|max:50',
            'nomor_penawaran' => 'nullable|string|max:50',
            'tanggal_penawaran' => 'nullable|date',
            'nilai_kontrak' => 'nullable|numeric|min:0',
            'tgl_sppbj' => 'nullable|date',
            'tgl_spk' => 'nullable|date',
            'tgl_spmk' => 'nullable|date',
            'tgl_selesai' => 'nullable|date',
            'sppbj' => 'nullable|string|max:50',
            'spk' => 'nullable|string|max:50',
            'spmk' => 'nullable|string|max:50',
        ]);

        $pekerjaanIds = $validated['pekerjaan_ids'] ?? [];
        if (empty($pekerjaanIds) && ! empty($validated['id_pekerjaan'])) {
            $pekerjaanIds = [(int) $validated['id_pekerjaan']];
        }

        if (empty($pekerjaanIds)) {
            return response()->json(['message' => 'Minimal satu pekerjaan harus dipilih'], 422);
        }

        $validated['id_pekerjaan'] = $validated['id_pekerjaan'] ?? $pekerjaanIds[0];
        $kontrak = Kontrak::create($validated);
        
        $kontrak->pekerjaans()->sync($pekerjaanIds);

        $kontrak->load('kegiatan', 'pekerjaans', 'penyedia');

        return new KontrakDetailResource($kontrak);
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak/{id}",
     *     summary="Get kontrak detail",
     *     tags={"Kontrak"},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Kontrak $kontrak)
    {
        $kontrak->load([
            'kegiatan',
            'pekerjaans.kecamatan',
            'pekerjaans.desa',
            'pekerjaans.kegiatan',
            'penyedia',
            'latestApprovedAddendum',
            'addendums.items',
            'addendums.media',
        ]);

        return new KontrakDetailResource($kontrak);
    }

    public function exportDoc(Kontrak $kontrak)
    {
        try {
            $kontrak->loadMissing(['kegiatan', 'pekerjaans.kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.desa', 'penyedia', 'approvedAddendums']);

            $path = $this->exportService->export($kontrak);

            return response()->download($path)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => $e->getMessage()], 500);
        }
    }

    public function exportRingkasan(Kontrak $kontrak)
    {
        $kontrak->loadMissing(['kegiatan', 'pekerjaans.kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.desa', 'penyedia', 'approvedAddendums']);

        $firstPekerjaan = $kontrak->pekerjaans->first();
        if ($firstPekerjaan && ! $firstPekerjaan->isChecklistComplete()) {
            return response()->json(['message' => 'Checklist pekerjaan belum 100% lengkap bos!'], 403);
        }

        try {
            $path = $this->exportService->exportRingkasan($kontrak);
            $namaPaket = $firstPekerjaan?->nama_paket ?? 'Kontrak';
            $downloadName = 'Ringkasan_'.Str::slug($namaPaket).'.xlsx';

            return response()->download($path, $downloadName)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => $e->getMessage()], 500);
        }
    }

    public function previewRingkasan(Kontrak $kontrak)
    {
        $kontrak->loadMissing(['kegiatan', 'pekerjaans.kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.desa', 'penyedia', 'approvedAddendums']);

        $firstPekerjaan = $kontrak->pekerjaans->first();
        if ($firstPekerjaan && ! $firstPekerjaan->isChecklistComplete()) {
            return response()->json(['message' => 'Checklist pekerjaan belum 100% lengkap bos!'], 403);
        }

        try {
            $path = $this->exportService->exportRingkasan($kontrak);
            $namaPaket = $firstPekerjaan?->nama_paket ?? 'Kontrak';
            $downloadName = 'Ringkasan_'.Str::slug($namaPaket).'.xlsx';

            $media = $kontrak
                ->addMedia($path)
                ->usingFileName($downloadName)
                ->toMediaCollection('kontrak/ringkasan-preview');

            if (file_exists($path)) {
                unlink($path);
            }

            return response()->json([
                'data' => [
                    'media_id' => $media->id,
                    'file_name' => $media->file_name,
                    'title' => 'Ringkasan Kontrak - '.$namaPaket,
                ],
            ]);
        } catch (\Exception $e) {
            return response()->json(['message' => $e->getMessage()], 500);
        }
    }

    public function exportCover(Kontrak $kontrak)
    {
        $kontrak->loadMissing(['kegiatan', 'pekerjaans.kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.desa', 'penyedia', 'approvedAddendums']);

        try {
            $path = $this->exportService->exportCover($kontrak);

            return response()->download($path)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => $e->getMessage()], 500);
        }
    }

    public function exportBAP(Request $request, Kontrak $kontrak)
    {
        $kontrak->loadMissing(['kegiatan', 'pekerjaans.kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.desa', 'penyedia', 'approvedAddendums']);

        $firstPekerjaan = $kontrak->pekerjaans->first();
        if ($firstPekerjaan && ! $firstPekerjaan->isChecklistComplete()) {
            return response()->json(['message' => 'Checklist pekerjaan belum 100% lengkap bos!'], 403);
        }

        try {
            // Urutan argumen: $kontrak, $format, $overrideData
            $path = $this->exportService->exportBAP($kontrak, 'docx', $request->all());

            return response()->download($path)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => $e->getMessage()], 500);
        }
    }

    /**
     * @OA\Put(
     *     path="/api/kontrak/{id}",
     *     summary="Update kontrak",
     *     tags={"Kontrak"},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *
     *     @OA\Response(response=200, description="Kontrak updated")
     * )
     */
    public function update(Request $request, Kontrak $kontrak)
    {
        $validated = $request->validate([
            'id_kegiatan' => 'nullable|integer|exists:tbl_kegiatan,id',
            'pekerjaan_ids' => 'nullable|array',
            'pekerjaan_ids.*' => 'integer|exists:tbl_pekerjaan,id',
            'id_pekerjaan' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'id_penyedia' => 'nullable|integer|exists:tbl_penyedia,id',
            'kode_rup' => 'nullable|string|max:50',
            'kode_paket' => 'nullable|string|max:50',
            'nomor_penawaran' => 'nullable|string|max:50',
            'tanggal_penawaran' => 'nullable|date',
            'nilai_kontrak' => 'nullable|numeric|min:0',
            'tgl_sppbj' => 'nullable|date',
            'tgl_spk' => 'nullable|date',
            'tgl_spmk' => 'nullable|date',
            'tgl_selesai' => 'nullable|date',
            'sppbj' => 'nullable|string|max:50',
            'spk' => 'nullable|string|max:50',
            'spmk' => 'nullable|string|max:50',
        ]);

        $kontrak->update($validated);

        $pekerjaanIds = $validated['pekerjaan_ids'] ?? [];
        if (empty($pekerjaanIds) && ! empty($validated['id_pekerjaan'])) {
            $pekerjaanIds = [(int) $validated['id_pekerjaan']];
        }
        if (empty($validated['id_pekerjaan']) && ! empty($pekerjaanIds)) {
            $validated['id_pekerjaan'] = $pekerjaanIds[0];
        }
        if (! empty($pekerjaanIds)) {
            $kontrak->pekerjaans()->sync($pekerjaanIds);
        }
        
        $kontrak->load('kegiatan', 'pekerjaans', 'penyedia');

        return new KontrakDetailResource($kontrak);
    }

    /**
     * @OA\Delete(
     *     path="/api/kontrak/{id}",
     *     summary="Delete kontrak",
     *     tags={"Kontrak"},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *
     *     @OA\Response(response=200, description="Kontrak deleted")
     * )
     */
    public function destroy(Kontrak $kontrak)
    {
        $kontrak->delete();

        return response()->json(['message' => 'Kontrak deleted successfully'], 200);
    }

    // Additional filters by relation

    public function byPekerjaan($pekerjaanId)
    {
        $kontrak = Kontrak::whereHas('pekerjaans', function($q) use ($pekerjaanId) {
            $q->whereKey($pekerjaanId);
        })->with('kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.kegiatan', 'penyedia')->paginate(20);

        return KontrakResource::collection($kontrak);
    }

    public function byKegiatan($kegiatanId)
    {
        $kontrak = Kontrak::where('id_kegiatan', $kegiatanId)
            ->with('kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.kegiatan', 'penyedia')
            ->paginate(20);

        return KontrakResource::collection($kontrak);
    }

    public function byPenyedia($penyediaId)
    {
        $kontrak = Kontrak::where('id_penyedia', $penyediaId)
            ->with('kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.kegiatan', 'penyedia')
            ->paginate(20);

        return KontrakResource::collection($kontrak);
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak/{id}/export",
     *     summary="Export contract in Word/PDF",
     *     tags={"Kontrak"},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="format", in="query", @OA\Schema(type="string", enum={"docx", "pdf"})),
     *
     *     @OA\Response(response=200, description="File download")
     * )
     */
    public function export(Request $request, $id)
    {
        // Try finding by direct contract ID first
        $kontrak = Kontrak::find($id);

        // If not found, try finding by pekerjaan_id
        if (! $kontrak) {
            $kontrak = Kontrak::whereHas('pekerjaans', function($q) use ($id) {
                $q->whereKey($id);
            })->latest()->first();
        }

        if (! $kontrak) {
            return response()->json(['message' => 'Kontrak not found'], 404);
        }

        $pekerjaan = $kontrak->pekerjaans->first();
        $format = $request->query('format', 'docx');

        try {
            $kontrak->loadMissing(['kegiatan', 'pekerjaans.kegiatan', 'pekerjaans.kecamatan', 'pekerjaans.desa', 'penyedia', 'approvedAddendums']);

            $path = $this->exportService->export($kontrak, $format);

            return response()->download($path)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => 'Export failed: '.$e->getMessage()], 500);
        }
    }

    public function downloadTemplate(Request $request)
    {
        return Excel::download(new KontrakTemplateExport($request->tahun), 'template_kontrak.xlsx');
    }

    public function import(Request $request)
    {
        $request->validate([
            'file' => 'required|mimes:xlsx,xls,csv',
        ]);

        try {
            $import = new KontrakImport;
            Excel::import($import, $request->file('file'));

            $failures = collect($import->failures())->map(function ($f) {
                return [
                    'row' => $f['row'] ?? null,
                    'message' => is_array($f['errors']) ? implode(', ', $f['errors']) : $f['errors'],
                    'values' => $f['values'] ?? null,
                    'debug' => $f['debug'] ?? null,
                ];
            });

            return response()->json([
                'message' => 'Import selesai',
                'success_count' => $import->importedRows,
                'error_count' => count($failures),
                'errors' => $failures,
                'debug' => [
                    'total_rows_excel' => $import->rows,
                    'total_skipped' => count($import->skippedRows ?? []),
                ],
            ]);
        } catch (\Exception $e) {
            return response()->json(['message' => 'Gagal mengimport kontrak: '.$e->getMessage()], 500);
        }
    }

    public function exportExcel(Request $request)
    {
        return Excel::download(new KontrakExport($request->tahun, $request->search), 'data_kontrak.xlsx');
    }
}
