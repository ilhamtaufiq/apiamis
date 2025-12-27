<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\Foto;
use App\Models\Kontrak;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class DataQualityController extends Controller
{
    /**
     * Get data quality statistics for administrators.
     */
    public function getStats(Request $request)
    {
        $tahun = $request->query('tahun');

        $baseQuery = Pekerjaan::query();

        if ($tahun) {
            $baseQuery->whereHas('kegiatan', function ($query) use ($tahun) {
                $query->where('tahun_anggaran', $tahun);
            });
        }

        // 1. Jobs without coordinates
        $noCoordsCount = (clone $baseQuery)->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                  ->from('tbl_foto')
                  ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id')
                  ->whereNotNull('koordinat');
        })->count();

        // 2. Jobs without photos
        $noPhotosCount = (clone $baseQuery)->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                  ->from('tbl_foto')
                  ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id');
        })->count();

        // 3. Jobs with physical progress > 0 but no photos
        $startedNoPhotosCount = (clone $baseQuery)->whereExists(function ($query) {
             $query->select(DB::raw(1))
                   ->from('tbl_kontrak')
                   ->whereRaw('tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id');
        })->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                  ->from('tbl_foto')
                  ->whereRaw('tbl_foto.pekerjaan_id = tbl_pekerjaan.id');
        })->count();

        // 4. Jobs without associated contracts
        $noContractCount = (clone $baseQuery)->whereNotExists(function ($query) {
            $query->select(DB::raw(1))
                  ->from('tbl_kontrak')
                  ->whereRaw('tbl_kontrak.id_pekerjaan = tbl_pekerjaan.id');
        })->count();

        return response()->json([
            'success' => true,
            'data' => [
                'no_coordinates' => $noCoordsCount,
                'no_photos' => $noPhotosCount,
                'started_no_photos' => $startedNoPhotosCount,
                'no_contracts' => $noContractCount,
                'total_jobs' => (clone $baseQuery)->count(),
            ]
        ]);
    }
}
