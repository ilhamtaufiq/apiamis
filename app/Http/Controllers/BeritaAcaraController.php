<?php

namespace App\Http\Controllers;

use App\Models\BeritaAcara;
use App\Models\Pekerjaan;
use App\Http\Resources\BeritaAcaraResource;
use Illuminate\Http\Request;

class BeritaAcaraController extends Controller
{
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

        $beritaAcara = BeritaAcara::updateOrCreate(
            ['pekerjaan_id' => $pekerjaanId],
            ['data' => $validated['data']]
        );

        return new BeritaAcaraResource($beritaAcara);
    }
}
