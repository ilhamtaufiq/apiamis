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
     * Generate next document number for contract related docs
     */
    public function generateNumber(Request $request)
    {
        $validated = $request->validate([
            'type' => 'required|string|in:sppbj,spk,spmk',
            'year' => 'nullable|integer',
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'kontrak_id' => 'nullable|integer'
        ]);

        $nomor = $this->baService->generateNextNumber(
            $validated['type'], 
            $validated['year'] ?? null,
            $validated['pekerjaan_id'],
            $validated['kontrak_id'] ?? null,
            false // <--- NEVER SAVE DURING GENERATE PREVIEW
        );

        return response()->json(['nomor' => $nomor]);
    }

    public function index(Request $request)
    {
        $query = Kontrak::with('kegiatan', 'pekerjaan', 'penyedia');

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

    public function show(Kontrak $kontrak)
    {
        $kontrak->load('kegiatan', 'pekerjaan', 'penyedia');
        return new KontrakDetailResource($kontrak);
    }

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
     * Export contract document
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
        
        try {
            $path = $this->exportService->exportKontrak($pekerjaan);
            return response()->download($path)->deleteFileAfterSend(true);
        } catch (\Exception $e) {
            return response()->json(['message' => 'Export failed: ' . $e->getMessage()], 500);
        }
    }
}
