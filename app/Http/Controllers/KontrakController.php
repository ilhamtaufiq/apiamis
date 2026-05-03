<?php

namespace App\Http\Controllers;

use App\Models\Kontrak;
use App\Http\Resources\KontrakResource;
use App\Http\Resources\KontrakDetailResource;
use Illuminate\Http\Request;

class KontrakController extends Controller
{
    protected $baService;
    protected $exportService;

    public function __construct(\App\Services\BeritaAcaraService $baService, \App\Services\DocumentExportService $exportService)
    {
        $this->baService = $baService;
        $this->exportService = $exportService;
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak/generate-number",
     *     summary="Generate next document number (Preview)",
     *     tags={"Kontrak"},
     *     @OA\Parameter(name="type", in="query", required=true, @OA\Schema(type="string", enum={"sppbj", "spk", "spmk"})),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Number generated")
     * )
     */
    public function generateNumber(Request $request)
    {
        $validated = $request->validate([
            'type' => 'required|string|in:sppbj,spk,spk_add,spmk,ba_lpp,ba_php,ba_stp,ba_final,stp_a,stp_b',
            'year' => 'nullable|integer',
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'kontrak_id' => 'nullable|integer'
        ]);

        $nomor = $this->baService->generateNextNumber(
            $validated['type'], 
            $validated['year'] ?? null,
            $validated['pekerjaan_id'],
            $validated['kontrak_id'] ?? null,
            true // SAVE TO DB IMMEDIATELY
        );

        return response()->json(['nomor' => $nomor]);
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak",
     *     summary="List all kontrak",
     *     tags={"Kontrak"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Kontrak::with(['kegiatan', 'pekerjaan.kecamatan', 'pekerjaan.kegiatan', 'penyedia']);

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }
        
        // Search functionality
        if ($request->has('search') && $request->search) {
            $search = $request->search;
            $query->where(function($q) use ($search) {
                $q->where('kode_rup', 'like', "%{$search}%")
                  ->orWhere('nomor_penawaran', 'like', "%{$search}%")
                  ->orWhere('kode_paket', 'like', "%{$search}%")
                  ->orWhereHas('pekerjaan', function($q) use ($search) {
                      $q->where('nama_paket', 'like', "%{$search}%");
                  })
                  ->orWhereHas('penyedia', function($q) use ($search) {
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
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"id_pekerjaan", "id_penyedia"},
     *             @OA\Property(property="id_pekerjaan", type="integer"),
     *             @OA\Property(property="id_penyedia", type="integer")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Kontrak created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'id_kegiatan' => 'nullable|integer|exists:tbl_kegiatan,id',
            'id_pekerjaan' => 'required|integer|exists:tbl_pekerjaan,id',
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

        $kontrak = Kontrak::create($validated);
        
        // Permanently commit the next sequence number now that it's successfully saved
        $year = $request->tgl_sppbj 
            ? date('Y', strtotime($request->tgl_sppbj)) 
            : ($request->tgl_spk ? date('Y', strtotime($request->tgl_spk)) : date('Y'));
        \App\Models\DocumentSequence::where('year', $year)->increment('last_number');
        
        $kontrak->load('kegiatan', 'pekerjaan', 'penyedia');
        return new KontrakDetailResource($kontrak);
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak/{id}",
     *     summary="Get kontrak detail",
     *     tags={"Kontrak"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Kontrak $kontrak)
    {
        $kontrak->load('kegiatan', 'pekerjaan', 'penyedia');
        return new KontrakDetailResource($kontrak);
    }

    /**
     * @OA\Put(
     *     path="/api/kontrak/{id}",
     *     summary="Update kontrak",
     *     tags={"Kontrak"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Kontrak updated")
     * )
     */
    public function update(Request $request, Kontrak $kontrak)
    {
        $validated = $request->validate([
            'id_kegiatan' => 'nullable|integer|exists:tbl_kegiatan,id',
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
        $kontrak->load('kegiatan', 'pekerjaan', 'penyedia');
        return new KontrakDetailResource($kontrak);
    }

    /**
     * @OA\Delete(
     *     path="/api/kontrak/{id}",
     *     summary="Delete kontrak",
     *     tags={"Kontrak"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
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
        $kontrak = Kontrak::where('id_pekerjaan', $pekerjaanId)->with('kegiatan', 'pekerjaan', 'penyedia')->paginate(20);
        return KontrakResource::collection($kontrak);
    }

    public function byKegiatan($kegiatanId)
    {
        $kontrak = Kontrak::where('id_kegiatan', $kegiatanId)->with('kegiatan', 'pekerjaan', 'penyedia')->paginate(20);
        return KontrakResource::collection($kontrak);
    }

    public function byPenyedia($penyediaId)
    {
        $kontrak = Kontrak::where('id_penyedia', $penyediaId)->with('kegiatan', 'pekerjaan', 'penyedia')->paginate(20);
        return KontrakResource::collection($kontrak);
    }

    /**
     * @OA\Get(
     *     path="/api/kontrak/{id}/export",
     *     summary="Export contract in Word/PDF",
     *     tags={"Kontrak"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="format", in="query", @OA\Schema(type="string", enum={"docx", "pdf"})),
     *     @OA\Response(response=200, description="File download")
     * )
     */
    public function export(Request $request, $id)
    {
        // Try finding by direct contract ID first
        $kontrak = Kontrak::find($id);
        
        // If not found, try finding by pekerjaan_id
        if (!$kontrak) {
             $kontrak = Kontrak::where('id_pekerjaan', $id)->latest()->first();
        }

        if (!$kontrak) {
            return response()->json(['message' => 'Kontrak not found'], 404);
        }

        $pekerjaan = $kontrak->pekerjaan;
        $format = $request->query('format', 'docx');
        
        try {
            $path = $this->exportService->exportKontrak($pekerjaan, null, $format);
            return response()->download($path)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => 'Export failed: ' . $e->getMessage()], 500);
        }
    }

    public function getLogs(Request $request)
    {
        $query = \DB::table('tbl_document_logs')
            ->leftJoin('tbl_pekerjaan', 'tbl_document_logs.id_pekerjaan', '=', 'tbl_pekerjaan.id')
            ->select('tbl_document_logs.*', 'tbl_pekerjaan.nama_paket as nama_paket');

        if ($request->has('search') && $request->search) {
            $search = $request->search;
            $query->where(function($q) use ($search) {
                $q->where('full_number', 'like', "%{$search}%")
                  ->orWhere('nama_paket', 'like', "%{$search}%");
            });
        }

        if ($request->has('type') && $request->type) {
            $query->where('tbl_document_logs.type', $request->type);
        }

        return response()->json([
            'data' => $query->orderBy('created_at', 'desc')->paginate(50)
        ]);
    }

    public function cancelLog($id)
    {
        \DB::table('tbl_document_logs')
            ->where('id', $id)
            ->update([
                'status' => 'canceled',
                'updated_at' => now()
            ]);

        return response()->json(['message' => 'Document number canceled']);
    }

    public function updateSequence(Request $request)
    {
        $validated = $request->validate([
            'type' => 'required|string',
            'year' => 'required|integer',
            'last_number' => 'required|integer|min:0'
        ]);

        \App\Models\DocumentSequence::updateOrCreate(
            ['type' => $validated['type'], 'year' => $validated['year']],
            ['last_number' => $validated['last_number']]
        );

        return response()->json(['message' => 'Sequence updated successfully']);
    }

    public function getSequences()
    {
        return response()->json([
            'data' => \App\Models\DocumentSequence::orderBy('year', 'desc')->get()
        ]);
    }
}
