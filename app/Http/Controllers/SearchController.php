<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use App\Models\Pekerjaan;
use App\Models\Kontrak;
use App\Models\Penyedia;
use App\Models\Kegiatan;
use App\Models\Desa;
use App\Models\User;
use App\Models\Foto;
use App\Models\Penerima;
use App\Models\Output;
use App\Models\Progress;

class SearchController extends Controller
{
    /**
     * Global Search
     * 
     * @OA\Get(
     *     path="/api/search",
     *     summary="Global Search",
     *     description="Search across multiple entities (Pekerjaan, Kontrak, Penyedia, Kegiatan, User, Desa) globally.",
     *     operationId="globalSearch",
     *     tags={"Search"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(
     *         name="q",
     *         in="query",
     *         description="Search query",
     *         required=true,
     *         @OA\Schema(type="string")
     *     ),
     *     @OA\Response(
     *         response=200,
     *         description="Search Results"
     *     )
     * )
     */
    public function index(Request $request)
    {
        $query = $request->query('q');

        if (!$query) {
            return response()->json([
                'success' => true,
                'data' => []
            ]);
        }

        $results = [];

        // 1. Search Pekerjaan
        $pekerjaan = Pekerjaan::byUserRole()
            ->where(function($q) use ($query) {
                $q->where('nama_paket', 'like', "%{$query}%")
                  ->orWhere('kode_rekening', 'like', "%{$query}%");
            })
            ->with(['desa', 'kecamatan'])
            ->limit(10)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Pekerjaan',
                    'title' => $item->nama_paket,
                    'subtitle' => $item->kode_rekening . ' - ' . ($item->desa->n_desa ?? ''),
                    'url' => "/pekerjaan/{$item->id}"
                ];
            });
        $results = array_merge($results, $pekerjaan->toArray());

        // 2. Search Kontrak
        $kontrak = Kontrak::where('spk', 'like', "%{$query}%")
            ->orWhere('spmk', 'like', "%{$query}%")
            ->orWhere('kode_paket', 'like', "%{$query}%")
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Kontrak',
                    'title' => 'Kontrak: ' . ($item->spk ?? $item->kode_paket),
                    'subtitle' => 'Pekerjaan ID: ' . $item->id_pekerjaan,
                    'url' => "/kontrak/{$item->id}"
                ];
            });
        $results = array_merge($results, $kontrak->toArray());

        // 3. Search Penyedia
        $penyedia = Penyedia::where('nama', 'like', "%{$query}%")
            ->orWhere('direktur', 'like', "%{$query}%")
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Penyedia',
                    'title' => $item->nama,
                    'subtitle' => "Direktur: " . $item->direktur,
                    'url' => "/penyedia/{$item->id}"
                ];
            });
        $results = array_merge($results, $penyedia->toArray());

        // 4. Search Kegiatan
        $kegiatan = Kegiatan::where('nama_kegiatan', 'like', "%{$query}%")
            ->orWhere('nama_sub_kegiatan', 'like', "%{$query}%")
            ->orWhere('nama_program', 'like', "%{$query}%")
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Kegiatan',
                    'title' => $item->nama_kegiatan,
                    'subtitle' => $item->nama_sub_kegiatan,
                    'url' => "/kegiatan/{$item->id}" // assuming valid url in frontend
                ];
            });
        $results = array_merge($results, $kegiatan->toArray());

        // 5. Search Desa
        $desa = Desa::where('n_desa', 'like', "%{$query}%")
            ->with('kecamatan')
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Desa',
                    'title' => 'Desa ' . $item->n_desa,
                    'subtitle' => 'Kec. ' . ($item->kecamatan->nama_kecamatan ?? ''),
                    'url' => "/desa/{$item->id}"
                ];
            });
        $results = array_merge($results, $desa->toArray());

        // 6. Search Dokumentasi (Foto)
        $dokumentasi = Foto::where('keterangan', 'like', "%{$query}%")
            ->with('pekerjaan') // For referencing parent pekerjaan
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Dokumentasi',
                    'title' => 'Dokumentasi: ' . ($item->keterangan ?? 'Tanpa Keterangan'),
                    'subtitle' => 'Pekerjaan: ' . ($item->pekerjaan->nama_paket ?? ''),
                    'url' => "/pekerjaan/{$item->pekerjaan_id}" // Generally points back to project
                ];
            });
        $results = array_merge($results, $dokumentasi->toArray());

        // 7. Search Penerima Manfaat
        $penerima = Penerima::where('nama', 'like', "%{$query}%")
            ->orWhere('nik', 'like', "%{$query}%")
            ->orWhere('alamat', 'like', "%{$query}%")
            ->with('pekerjaan')
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Penerima Manfaat',
                    'title' => 'Penerima: ' . $item->nama,
                    'subtitle' => 'Alamat: ' . ($item->alamat ?? '') . ' | Pekerjaan: ' . ($item->pekerjaan->nama_paket ?? ''),
                    'url' => "/pekerjaan/{$item->pekerjaan_id}" 
                ];
            });
        $results = array_merge($results, $penerima->toArray());

        // 8. Search Output
        $output = Output::where('komponen', 'like', "%{$query}%")
            ->orWhere('satuan', 'like', "%{$query}%")
            ->with('pekerjaan')
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Output',
                    'title' => 'Output: ' . $item->komponen,
                    'subtitle' => 'Volume: ' . $item->volume . ' ' . $item->satuan . ' | Pekerjaan: ' . ($item->pekerjaan->nama_paket ?? ''),
                    'url' => "/pekerjaan/{$item->pekerjaan_id}"
                ];
            });
        $results = array_merge($results, $output->toArray());

        // 9. Search Progress
        $progress = Progress::where('content', 'like', "%{$query}%") // assumes content column can be queried directly if JSON cast allows LIKE or it contains string matches
            ->with('pekerjaan')
            ->limit(5)
            ->get()
            ->map(function ($item) {
                return [
                    'id' => $item->id,
                    'type' => 'Progress',
                    'title' => 'Progress Log Entry',
                    'subtitle' => 'Pekerjaan: ' . ($item->pekerjaan->nama_paket ?? ''),
                    'url' => "/pekerjaan/{$item->pekerjaan_id}"
                ];
            });
        $results = array_merge($results, $progress->toArray());

        return response()->json([
            'success' => true,
            'data' => collect($results)->sortBy('type')->values()->all()
        ]);
    }
}
