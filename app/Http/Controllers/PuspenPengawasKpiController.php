<?php

namespace App\Http\Controllers;

use App\Models\Pengawas;
use App\Models\Pekerjaan;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class PuspenPengawasKpiController extends Controller
{
    public function index(Request $request)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'search' => 'nullable|string|max:100',
            'per_page' => 'nullable|integer|min:1|max:100',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);
        $search = $validated['search'] ?? null;
        $perPage = (int) ($validated['per_page'] ?? 20);

        // Base query for pengawas with their supervised pekerjaan
        $pengawasQuery = Pengawas::query();

        if ($search) {
            $pengawasQuery->where(function ($q) use ($search) {
                $q->where('nama', 'like', "%{$search}%")
                  ->orWhere('nip', 'like', "%{$search}%");
            });
        }

        $pengawas = $pengawasQuery->get();

        $results = [];

        foreach ($pengawas as $p) {
            // Get all pekerjaan IDs supervised by this pengawas (as pengawas or pendamping)
            $pekerjaanIds = Pekerjaan::query()
                ->where('pengawas_id', $p->id)
                ->orWhere('pendamping_id', $p->id)
                ->pluck('id');

            $pekerjaanCount = $pekerjaanIds->count();

            if ($pekerjaanCount === 0) {
                continue;
            }

            // Counts from related tables — only the sections pengawas actually update in Pekerjaan Detail:
            // Output, Penerima, Foto, Progress Fisik (the Laporan Progress Fisik tab)
            $fotoCount = DB::table('tbl_foto')
                ->whereIn('pekerjaan_id', $pekerjaanIds)
                ->count();

            $penerimaCount = DB::table('tbl_penerima')
                ->whereIn('pekerjaan_id', $pekerjaanIds)
                ->count();

            $outputCount = DB::table('tbl_output')
                ->whereIn('pekerjaan_id', $pekerjaanIds)
                ->count();

            // Progress Fisik: detailed updates in the Progress tab (tbl_progress.content contains weekly fisik items)
            // Count of progress reports = proxy for how much fisik data has been inputted/updated by pengawas
            $fisikCount = DB::table('tbl_progress')
                ->whereIn('pekerjaan_id', $pekerjaanIds)
                ->count();

            // Composite score focused on the 4 inputs mentioned:
            // Foto (documentation), Penerima (beneficiaries), Output (physical outputs), Progress Fisik
            // Weights: foto 1, penerima 1, output 2 (important deliverables), fisik 2 (core progress)
            $score = ($fotoCount * 1) +
                     ($penerimaCount * 1) +
                     ($outputCount * 2) +
                     ($fisikCount * 2);

            $results[] = [
                'id' => $p->id,
                'nama' => $p->nama,
                'nip' => $p->nip,
                'jabatan' => $p->jabatan,
                'pekerjaan_count' => $pekerjaanCount,
                'foto_count' => $fotoCount,
                'penerima_count' => $penerimaCount,
                'output_count' => $outputCount,
                // Progress Fisik count (from the detailed "Laporan Progress Fisik" tab in Pekerjaan Detail)
                'fisik_count' => $fisikCount,
                'score' => round($score, 1),
            ];
        }

        // Sort by score desc, then by pekerjaan_count
        usort($results, function ($a, $b) {
            if ($b['score'] === $a['score']) {
                return $b['pekerjaan_count'] <=> $a['pekerjaan_count'];
            }
            return $b['score'] <=> $a['score'];
        });

        // Assign rank
        foreach ($results as $index => &$row) {
            $row['rank'] = $index + 1;
        }

        // Apply search already done, now paginate manually (simple)
        $total = count($results);
        $page = max(1, (int) $request->get('page', 1));
        $offset = ($page - 1) * $perPage;
        $paginated = array_slice($results, $offset, $perPage);

        return response()->json([
            'data' => $paginated,
            'meta' => [
                'current_page' => $page,
                'last_page' => max(1, (int) ceil($total / $perPage)),
                'per_page' => $perPage,
                'total' => $total,
            ],
            'summary' => [
                'total_pengawas' => count($results),
                'tahun' => $tahun,
            ],
        ]);
    }
}
