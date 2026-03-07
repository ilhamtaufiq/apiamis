<?php

namespace App\Http\Controllers;

use App\Models\BeritaAcara;
use App\Models\Pekerjaan;
use App\Http\Resources\BeritaAcaraResource;
use Illuminate\Http\Request;

class BeritaAcaraController extends Controller
{
    protected $baService;

    public function __construct(\App\Services\BeritaAcaraService $baService)
    {
        $this->baService = $baService;
    }

    public function getSequence(Request $request)
    {
        $year = $request->query('year', date('Y'));
        $sequence = \App\Models\DocumentSequence::firstOrCreate(
            ['year' => $year],
            ['last_number' => 0]
        );
        return response()->json($sequence);
    }

    public function updateSequence(Request $request)
    {
        $validated = $request->validate([
            'year' => 'required|integer',
            'last_number' => 'required|integer|min:0'
        ]);

        $sequence = \App\Models\DocumentSequence::updateOrCreate(
            ['year' => $validated['year']],
            ['last_number' => $validated['last_number']]
        );

        return response()->json($sequence);
    }

    /**
     * Get berita acara by pekerjaan ID
     */
    public function show($pekerjaanId)
    {
        $pekerjaan = Pekerjaan::findOrFail($pekerjaanId);
        
        $beritaAcara = BeritaAcara::firstOrCreate(
            ['pekerjaan_id' => $pekerjaanId],
            ['data' => BeritaAcara::getDefaultData()]
        );

        return new BeritaAcaraResource($beritaAcara);
    }

    /**
     * Generate next document number
     */
    public function generateNumber(Request $request)
    {
        $validated = $request->validate([
            'type' => 'required|string|in:ba_lpp,stp_a,stp_b,ba_php,ba_stp',
            'year' => 'nullable|integer',
            'pekerjaan_id' => 'nullable|integer|exists:tbl_pekerjaan,id'
        ]);

        $nomor = $this->baService->generateNextNumber(
            $validated['type'], 
            $validated['year'] ?? null,
            $validated['pekerjaan_id'] ?? null,
            null,
            false // <--- NEVER SAVE DURING GENERATE PREVIEW
        );

        return response()->json(['nomor' => $nomor]);
    }

    /**
     * Store or update berita acara for a pekerjaan
     */
    public function storeOrUpdate(Request $request, $pekerjaanId)
    {
        $pekerjaan = Pekerjaan::findOrFail($pekerjaanId);

        $validated = $request->validate([
            'data' => 'required|array',
            'data.ba_lpp' => 'nullable|array',
            'data.ba_lpp.*.nomor' => 'required|string',
            'data.ba_lpp.*.tanggal' => 'required|date',
            'data.serah_terima_pertama' => 'nullable|array',
            'data.serah_terima_pertama.*.nomor' => 'required|string',
            'data.serah_terima_pertama.*.tanggal' => 'required|date',
            'data.ba_php' => 'nullable|array',
            'data.ba_php.*.nomor' => 'required|string',
            'data.ba_php.*.tanggal' => 'required|date',
            'data.ba_stp' => 'nullable|array',
            'data.ba_stp.*.nomor' => 'required|string',
            'data.ba_stp.*.tanggal' => 'required|date',
        ]);

        $beritaAcara = BeritaAcara::where('pekerjaan_id', $pekerjaanId)->first();
        if (!$beritaAcara) {
            \App\Models\DocumentSequence::where('year', date('Y'))->increment('last_number');
        }

        $beritaAcara = BeritaAcara::updateOrCreate(
            ['pekerjaan_id' => $pekerjaanId],
            ['data' => $validated['data']]
        );

        return new BeritaAcaraResource($beritaAcara);
    }
}
