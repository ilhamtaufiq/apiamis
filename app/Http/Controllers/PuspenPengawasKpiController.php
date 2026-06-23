<?php

namespace App\Http\Controllers;

use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;
use Spatie\Permission\Models\Role;

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

        $results = $this->buildResults($tahun, $search);

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

    public function show(Request $request, int $userId)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);

        $user = User::whereHas('roles', function ($q) {
            $q->where('name', 'pengawas');
        })->findOrFail($userId);

        $pekerjaanRows = $this->pekerjaanBreakdownForUser($user, $tahun);

        $totals = [
            'pekerjaan_count' => $pekerjaanRows->count(),
            'foto_count' => (int) $pekerjaanRows->sum('foto_count'),
            'penerima_count' => (int) $pekerjaanRows->sum('penerima_count'),
            'output_count' => (int) $pekerjaanRows->sum('output_count'),
            'fisik_count' => (int) $pekerjaanRows->sum('fisik_count'),
            'score' => round((float) $pekerjaanRows->sum('score'), 1),
        ];

        $totals['score_per_pekerjaan'] = $totals['pekerjaan_count'] > 0
            ? round($totals['score'] / $totals['pekerjaan_count'], 1)
            : 0.0;

        return response()->json([
            'user' => [
                'id' => $user->id,
                'nama' => $user->name,
                'nip' => $user->nip,
            ],
            'tahun' => $tahun,
            'pekerjaan' => $pekerjaanRows->values(),
            'summary' => $totals,
        ]);
    }

    private function buildResults(int $tahun, ?string $search): array
    {
        Role::firstOrCreate(['name' => 'pengawas']);

        $assignedUserIds = DB::table('user_pekerjaan')->distinct()->pluck('user_id');

        if ($assignedUserIds->isNotEmpty()) {
            $usersToGrant = User::whereIn('id', $assignedUserIds)
                ->get()
                ->filter(fn ($u) => ! $u->hasRole('pengawas'));

            foreach ($usersToGrant as $u) {
                $u->assignRole('pengawas');
            }
        }

        $userQuery = User::whereHas('roles', function ($q) {
            $q->where('name', 'pengawas');
        });

        if ($search) {
            $userQuery->where(function ($q) use ($search) {
                $q->where('name', 'like', "%{$search}%")
                    ->orWhere('nip', 'like', "%{$search}%")
                    ->orWhere('email', 'like', "%{$search}%");
            });
        }

        $users = $userQuery->get();
        $results = [];

        foreach ($users as $u) {
            $pekerjaanRows = $this->pekerjaanBreakdownForUser($u, $tahun);

            if ($pekerjaanRows->isEmpty()) {
                continue;
            }

            $pekerjaanCount = $pekerjaanRows->count();
            $fotoCount = (int) $pekerjaanRows->sum('foto_count');
            $penerimaCount = (int) $pekerjaanRows->sum('penerima_count');
            $outputCount = (int) $pekerjaanRows->sum('output_count');
            $fisikCount = (int) $pekerjaanRows->sum('fisik_count');
            $score = round((float) $pekerjaanRows->sum('score'), 1);
            $scorePerPekerjaan = $pekerjaanCount > 0
                ? round($score / $pekerjaanCount, 1)
                : 0.0;

            $results[] = [
                'id' => $u->id,
                'nama' => $u->name,
                'nip' => $u->nip,
                'jabatan' => $u->jabatan,
                'pekerjaan_count' => $pekerjaanCount,
                'foto_count' => $fotoCount,
                'penerima_count' => $penerimaCount,
                'output_count' => $outputCount,
                'fisik_count' => $fisikCount,
                'score' => $score,
                'score_per_pekerjaan' => $scorePerPekerjaan,
            ];
        }

        usort($results, function ($a, $b) {
            if ($b['score_per_pekerjaan'] === $a['score_per_pekerjaan']) {
                if ($b['score'] === $a['score']) {
                    return $b['pekerjaan_count'] <=> $a['pekerjaan_count'];
                }

                return $b['score'] <=> $a['score'];
            }

            return $b['score_per_pekerjaan'] <=> $a['score_per_pekerjaan'];
        });

        foreach ($results as $index => &$row) {
            $row['rank'] = $index + 1;
        }

        return $results;
    }

    private function pekerjaanBreakdownForUser(User $user, int $tahun): Collection
    {
        $pekerjaanList = $user->assignedPekerjaan()
            ->whereHas('kegiatan', function ($q) use ($tahun) {
                $q->where('tahun_anggaran', $tahun);
            })
            ->select('tbl_pekerjaan.id', 'tbl_pekerjaan.nama_paket', 'tbl_pekerjaan.kode_rekening')
            ->get();

        return $pekerjaanList->map(function ($pekerjaan) {
            $pekerjaanId = $pekerjaan->id;

            $fotoCount = DB::table('tbl_foto')->where('pekerjaan_id', $pekerjaanId)->count();
            $penerimaCount = DB::table('tbl_penerima')->where('pekerjaan_id', $pekerjaanId)->count();
            $outputCount = DB::table('tbl_output')->where('pekerjaan_id', $pekerjaanId)->count();
            $fisikCount = DB::table('tbl_progress')->where('pekerjaan_id', $pekerjaanId)->count();

            $score = ($fotoCount * 1) +
                ($penerimaCount * 1) +
                ($outputCount * 2) +
                ($fisikCount * 2);

            return [
                'id' => $pekerjaanId,
                'nama_paket' => $pekerjaan->nama_paket,
                'kode_rekening' => $pekerjaan->kode_rekening,
                'foto_count' => $fotoCount,
                'penerima_count' => $penerimaCount,
                'output_count' => $outputCount,
                'fisik_count' => $fisikCount,
                'score' => round($score, 1),
            ];
        })->sortByDesc('score')->values();
    }
}