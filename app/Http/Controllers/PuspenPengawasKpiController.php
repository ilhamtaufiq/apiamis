<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\User;
use App\Services\PekerjaanProgressEstimasiSummaryService;
use Illuminate\Http\Request;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Schema;
use Spatie\Permission\Models\Role;

class PuspenPengawasKpiController extends Controller
{
    /** @var list<string> */
    private const KPI_ROLES = ['pengawas', 'konsultan_pengawas'];

    public function index(Request $request)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'search' => 'nullable|string|max:100',
            'peran' => 'nullable|string|in:pengawas,konsultan_pengawas',
            'per_page' => 'nullable|integer|min:1|max:100',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);
        $search = $validated['search'] ?? null;
        $peran = $validated['peran'] ?? null;
        $perPage = (int) ($validated['per_page'] ?? 20);

        $results = $this->buildResults($tahun, $search, $peran);

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
                'peran' => $peran,
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
            $q->whereIn('name', self::KPI_ROLES);
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
                'roles' => $this->getUserKpiRoles($user),
            ],
            'tahun' => $tahun,
            'pekerjaan' => $pekerjaanRows->values(),
            'summary' => $totals,
        ]);
    }

    /**
     * Laporan catatan kelengkapan paket (untuk export PDF).
     * Kolom utama: nomor, nama paket, catatan (+ pengawas untuk multi-section).
     */
    public function notesReport(Request $request)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'search' => 'nullable|string|max:100',
            'peran' => 'nullable|string|in:pengawas,konsultan_pengawas',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);
        $search = $validated['search'] ?? null;
        $peran = $validated['peran'] ?? null;

        $results = $this->buildResults($tahun, $search, $peran);
        $rows = [];
        $no = 1;

        foreach ($results as $r) {
            $user = User::find($r['id']);
            if (! $user) {
                continue;
            }

            $pekerjaanRows = $this->pekerjaanBreakdownForUser($user, $tahun);
            foreach ($pekerjaanRows as $p) {
                $rows[] = [
                    'no' => $no++,
                    'pengawas' => $r['nama'],
                    'nip' => $r['nip'],
                    'pekerjaan_id' => $p['id'],
                    'nama_paket' => $p['nama_paket'],
                    'kode_rekening' => $p['kode_rekening'],
                    'catatan' => $p['catatan'] ?? '',
                    'progress_realisasi' => $p['progress_realisasi'] ?? null,
                    'pho_completed' => (bool) ($p['pho_completed'] ?? false),
                    'foto_count' => $p['foto_count'] ?? 0,
                    'penerima_count' => $p['penerima_count'] ?? 0,
                    'output_count' => $p['output_count'] ?? 0,
                ];
            }
        }

        return response()->json([
            'tahun' => $tahun,
            'peran' => $peran,
            'total' => count($rows),
            'data' => $rows,
        ]);
    }

    private function buildResults(int $tahun, ?string $search, ?string $peran): array
    {
        $this->ensureKpiRoles();

        $assignedUserIds = DB::table('user_pekerjaan')->distinct()->pluck('user_id');

        if ($assignedUserIds->isNotEmpty()) {
            User::whereIn('id', $assignedUserIds)
                ->get()
                ->each(fn (User $u) => $u->grantPengawasRoleIfEligible());
        }

        $roleFilter = $this->resolveRoleFilter($peran);

        $userQuery = User::whereHas('roles', function ($q) use ($roleFilter) {
            $q->whereIn('name', $roleFilter);
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
            // score per paket = 0–100 kelengkapan; total = jumlah skor paket (bukan volume spam)
            $score = round((float) $pekerjaanRows->sum('score'), 1);
            $scorePerPekerjaan = $pekerjaanCount > 0
                ? round($score / $pekerjaanCount, 1)
                : 0.0;
            $qualityPackages = $pekerjaanRows->filter(fn ($row) => ($row['score'] ?? 0) >= 70)->count();

            $results[] = [
                'id' => $u->id,
                'nama' => $u->name,
                'nip' => $u->nip,
                'jabatan' => $u->jabatan,
                'avatar' => $u->avatar,
                'roles' => $this->getUserKpiRoles($u),
                'pekerjaan_count' => $pekerjaanCount,
                'foto_count' => $fotoCount,
                'penerima_count' => $penerimaCount,
                'output_count' => $outputCount,
                'fisik_count' => $fisikCount,
                'score' => $score,
                'score_per_pekerjaan' => $scorePerPekerjaan,
                'quality_packages' => $qualityPackages,
            ];
        }

        // Ranking fair: rata-rata kelengkapan dulu, lalu jumlah paket berkualitas (≥70), baru total.
        usort($results, function ($a, $b) {
            if ($b['score_per_pekerjaan'] !== $a['score_per_pekerjaan']) {
                return $b['score_per_pekerjaan'] <=> $a['score_per_pekerjaan'];
            }

            $qualityA = (int) ($a['quality_packages'] ?? 0);
            $qualityB = (int) ($b['quality_packages'] ?? 0);
            if ($qualityB !== $qualityA) {
                return $qualityB <=> $qualityA;
            }

            if ($b['score'] !== $a['score']) {
                return $b['score'] <=> $a['score'];
            }

            return $b['pekerjaan_count'] <=> $a['pekerjaan_count'];
        });

        foreach ($results as $index => &$row) {
            $row['rank'] = $index + 1;
        }

        return $results;
    }

    /**
     * Bobot dimensi kelengkapan (0–100). Hanya dimensi yang applicable di-normalisasi.
     * Bukan volume mentah — mencegah gaming lewat spam foto/output.
     */
    private const SCORE_WEIGHT_FOTO = 35;

    private const SCORE_WEIGHT_PENERIMA = 25;

    private const SCORE_WEIGHT_PROGRESS = 25;

    private const SCORE_WEIGHT_KOORDINAT = 15;

    private function pekerjaanBreakdownForUser(User $user, int $tahun): Collection
    {
        $pekerjaanList = $user->assignedPekerjaan()
            ->notCanceled()
            ->whereHas('kegiatan', function ($q) use ($tahun) {
                $q->where('tahun_anggaran', $tahun);
            })
            ->with([
                'output',
                'foto',
                'progressEstimasiHistory',
                'kegiatan:id,tahun_anggaran',
                'kontrak' => function ($q) {
                    $q->select('tbl_kontrak.id', 'tbl_kontrak.id_pekerjaan');
                },
            ])
            ->withCount(['penerima', 'foto', 'output'])
            ->get();

        $estimasiService = app(PekerjaanProgressEstimasiSummaryService::class);
        $hasPhoColumn = Schema::hasTable('puspen_progress_fisik')
            && Schema::hasColumn('puspen_progress_fisik', 'pho_completed');

        return $pekerjaanList->map(function (Pekerjaan $pekerjaan) use ($tahun, $estimasiService, $hasPhoColumn) {
            $pekerjaanId = $pekerjaan->id;

            $fotoCount = (int) ($pekerjaan->foto_count ?? $pekerjaan->foto?->count() ?? 0);
            $penerimaCount = (int) ($pekerjaan->penerima_count ?? 0);
            $outputCount = (int) ($pekerjaan->output_count ?? $pekerjaan->output?->count() ?? 0);
            $fisikCount = (int) DB::table('tbl_progress')->where('pekerjaan_id', $pekerjaanId)->count();

            $estimasi = $estimasiService->summarize(
                $pekerjaan->progressEstimasiHistory ?? collect(),
                $tahun
            );
            $progressRealisasi = $estimasi['fisik_realisasi'];

            // Fallback: puspen_progress_fisik realisasi via kontrak
            if ($progressRealisasi === null) {
                $progressRealisasi = $this->resolvePuspenRealisasi($pekerjaanId, $tahun);
            }

            $phoCompleted = $hasPhoColumn
                ? $this->resolvePhoCompleted($pekerjaanId, $tahun)
                : false;

            $fotoMetrics = $pekerjaan->resolveFotoMetrics();
            $fotoCount = (int) ($fotoMetrics['foto_count'] ?? $fotoCount);
            $fotoRequired = $fotoMetrics['foto_required_count'];

            $scoreBreakdown = $this->computeFairPackageScore(
                isKonsultan: (bool) ($pekerjaan->is_konsultan ?? false),
                fotoCount: $fotoCount,
                fotoRequired: is_numeric($fotoRequired) ? (int) $fotoRequired : null,
                fotoStatus: $fotoMetrics['foto_status'] ?? null,
                penerimaCount: $penerimaCount,
                outputCount: $outputCount,
                outputs: $pekerjaan->output ?? collect(),
                progressRealisasi: $progressRealisasi,
                fisikCount: $fisikCount,
                phoCompleted: $phoCompleted,
                fotos: $pekerjaan->foto ?? collect(),
            );

            $catatan = $this->buildPekerjaanCatatan(
                progressRealisasi: $progressRealisasi,
                phoCompleted: $phoCompleted,
                fotoStatus: $fotoMetrics['foto_status'] ?? null,
                fotoCount: $fotoCount,
                fotoRequired: $fotoRequired,
                penerimaCount: $penerimaCount,
                outputCount: $outputCount,
                fisikCount: $fisikCount,
            );

            return [
                'id' => $pekerjaanId,
                'nama_paket' => $pekerjaan->nama_paket,
                'kode_rekening' => $pekerjaan->kode_rekening,
                'is_konsultan' => (bool) ($pekerjaan->is_konsultan ?? false),
                'foto_count' => $fotoCount,
                'penerima_count' => $penerimaCount,
                'output_count' => $outputCount,
                'fisik_count' => $fisikCount,
                /** Skor kelengkapan 0–100 (bukan volume). */
                'score' => $scoreBreakdown['score'],
                'score_breakdown' => $scoreBreakdown['breakdown'],
                'progress_realisasi' => $progressRealisasi,
                'pho_completed' => $phoCompleted,
                'foto_status' => $fotoMetrics['foto_status'] ?? null,
                'foto_required_count' => $fotoMetrics['foto_required_count'],
                'catatan' => $catatan,
            ];
        })->sortByDesc('score')->values();
    }

    /**
     * Skor fair 0–100: rasio kelengkapan berbobot, di-cap target (bukan jumlah mentah).
     *
     * @param  \Illuminate\Support\Collection|iterable  $outputs
     * @param  \Illuminate\Support\Collection|iterable  $fotos
     * @return array{score: float, breakdown: array<string, float|null>}
     */
    private function computeFairPackageScore(
        bool $isKonsultan,
        int $fotoCount,
        ?int $fotoRequired,
        ?string $fotoStatus,
        int $penerimaCount,
        int $outputCount,
        $outputs,
        ?float $progressRealisasi,
        int $fisikCount,
        bool $phoCompleted,
        $fotos,
    ): array {
        $outputs = collect($outputs);
        $fotos = collect($fotos);

        // --- Progress (selalu applicable) ---
        $progressRatio = 0.0;
        if ($progressRealisasi !== null && $progressRealisasi > 0) {
            $progressRatio = min(1.0, $progressRealisasi / 100.0);
        } elseif ($fisikCount > 0) {
            // Ada input progress mentah tanpa % estimasi → kredit terbatas
            $progressRatio = 0.25;
        }
        if ($phoCompleted) {
            $progressRatio = max($progressRatio, 0.9);
        }

        // Paket konsultan: penilaian fokusus progress (tanpa foto/penerima unit)
        if ($isKonsultan) {
            $score = round($progressRatio * 100, 1);

            return [
                'score' => $score,
                'breakdown' => [
                    'foto' => null,
                    'penerima' => null,
                    'progress' => round($progressRatio * 100, 1),
                    'koordinat' => null,
                ],
            ];
        }

        // --- Foto (cap ke target slot; butuh output) ---
        $fotoApplicable = $outputCount > 0 && $fotoRequired !== null && $fotoRequired > 0;
        $fotoRatio = 0.0;
        if ($fotoApplicable) {
            if ($fotoStatus === 'selesai') {
                $fotoRatio = 1.0;
            } else {
                $fotoRatio = min(1.0, $fotoCount / max(1, $fotoRequired));
            }
        }

        // --- Penerima (hanya output yang wajib unit/penerima) ---
        $penerimaTarget = 0;
        foreach ($outputs as $output) {
            $optional = (bool) ($output->penerima_is_optional ?? false);
            if ($optional) {
                continue;
            }
            $penerimaTarget += max(1, (int) ceil((float) ($output->volume ?? 0)));
        }
        $penerimaApplicable = $penerimaTarget > 0;
        $penerimaRatio = $penerimaApplicable
            ? min(1.0, $penerimaCount / $penerimaTarget)
            : 0.0;

        // --- Koordinat (dari foto yang sudah ada) ---
        $fotoWithCoords = $fotos->filter(function ($foto) {
            $k = $foto->koordinat ?? null;

            return is_string($k) ? trim($k) !== '' : ! empty($k);
        })->count();
        $koordinatApplicable = $fotoCount > 0;
        $koordinatRatio = $koordinatApplicable
            ? min(1.0, $fotoWithCoords / max(1, $fotoCount))
            : 0.0;

        // --- Output belum diisi: progress tetap dihitung, dimensi lain non-applicable ---
        if ($outputCount === 0) {
            // Tanpa output, foto/penerima tidak bisa dinilai — skor = progress saja (maks 100)
            // tapi diberi penalti 0.5 agar tidak setara paket lengkap
            $score = round($progressRatio * 100 * 0.5, 1);

            return [
                'score' => $score,
                'breakdown' => [
                    'foto' => null,
                    'penerima' => null,
                    'progress' => round($progressRatio * 100, 1),
                    'koordinat' => null,
                ],
            ];
        }

        $dimensions = [
            ['ratio' => $fotoRatio, 'weight' => self::SCORE_WEIGHT_FOTO, 'applicable' => $fotoApplicable, 'key' => 'foto'],
            ['ratio' => $penerimaRatio, 'weight' => self::SCORE_WEIGHT_PENERIMA, 'applicable' => $penerimaApplicable, 'key' => 'penerima'],
            ['ratio' => $progressRatio, 'weight' => self::SCORE_WEIGHT_PROGRESS, 'applicable' => true, 'key' => 'progress'],
            ['ratio' => $koordinatRatio, 'weight' => self::SCORE_WEIGHT_KOORDINAT, 'applicable' => $koordinatApplicable, 'key' => 'koordinat'],
        ];

        $applicableWeight = 0.0;
        $weightedSum = 0.0;
        $breakdown = [
            'foto' => null,
            'penerima' => null,
            'progress' => round($progressRatio * 100, 1),
            'koordinat' => null,
        ];

        foreach ($dimensions as $dim) {
            if (! $dim['applicable']) {
                continue;
            }
            $applicableWeight += $dim['weight'];
            $weightedSum += $dim['ratio'] * $dim['weight'];
            $breakdown[$dim['key']] = round($dim['ratio'] * 100, 1);
        }

        $score = $applicableWeight > 0
            ? round(($weightedSum / $applicableWeight) * 100, 1)
            : 0.0;

        return [
            'score' => $score,
            'breakdown' => $breakdown,
        ];
    }

    private function resolvePuspenRealisasi(int $pekerjaanId, int $tahun): ?float
    {
        if (! Schema::hasTable('puspen_progress_fisik')) {
            return null;
        }

        $kontrakIds = $this->kontrakIdsForPekerjaan($pekerjaanId);
        if ($kontrakIds === []) {
            return null;
        }

        $row = DB::table('puspen_progress_fisik')
            ->whereIn('kontrak_id', $kontrakIds)
            ->where('tahun_anggaran', $tahun)
            ->whereNotNull('realisasi')
            ->orderByDesc('updated_at')
            ->first();

        return $row ? (float) $row->realisasi : null;
    }

    private function resolvePhoCompleted(int $pekerjaanId, int $tahun): bool
    {
        if (! Schema::hasTable('puspen_progress_fisik')
            || ! Schema::hasColumn('puspen_progress_fisik', 'pho_completed')) {
            return false;
        }

        $kontrakIds = $this->kontrakIdsForPekerjaan($pekerjaanId);
        if ($kontrakIds === []) {
            return false;
        }

        return DB::table('puspen_progress_fisik')
            ->whereIn('kontrak_id', $kontrakIds)
            ->where('tahun_anggaran', $tahun)
            ->where('pho_completed', true)
            ->exists();
    }

    /** @return list<int> */
    private function kontrakIdsForPekerjaan(int $pekerjaanId): array
    {
        $fromPrimary = DB::table('tbl_kontrak')
            ->where('id_pekerjaan', $pekerjaanId)
            ->pluck('id');

        $fromPivot = Schema::hasTable('kontrak_pekerjaan')
            ? DB::table('kontrak_pekerjaan')
                ->where('pekerjaan_id', $pekerjaanId)
                ->pluck('kontrak_id')
            : collect();

        return $fromPrimary->merge($fromPivot)->unique()->values()->all();
    }

    /**
     * Catatan kelengkapan untuk laporan PDF.
     *
     * @param  int|null  $fotoRequired
     */
    private function buildPekerjaanCatatan(
        ?float $progressRealisasi,
        bool $phoCompleted,
        ?string $fotoStatus,
        int $fotoCount,
        $fotoRequired,
        int $penerimaCount,
        int $outputCount,
        int $fisikCount,
    ): string {
        $notes = [];
        $outputBelum = $outputCount === 0;
        $progressFull = $progressRealisasi !== null && $progressRealisasi >= 100;
        $fotoLengkap = $fotoStatus === 'selesai';
        $fotoBelum = $fotoStatus === 'belum_ada_foto' || $fotoCount === 0;
        $fotoPartial = ! $fotoLengkap && ! $fotoBelum;

        // Peringatan kritis dulu: progress/PHO tinggi tapi data pendukung kosong
        if ($outputBelum) {
            if ($progressFull || $phoCompleted) {
                $status = [];
                if ($progressFull) {
                    $status[] = 'progress fisik 100%';
                }
                if ($phoCompleted) {
                    $status[] = 'sudah PHO';
                }
                $missing = ['output komponen belum ditambahkan'];
                if ($fotoBelum) {
                    $missing[] = 'foto tidak ada';
                }
                if ($penerimaCount === 0) {
                    $missing[] = 'penerima tidak ada';
                }
                $notes[] = '[KRITIS] '.implode(' + ', $status)
                    .' tetapi '.implode(', ', $missing)
                    .' — kelengkapan foto diabaikan (slot foto mengikuti komponen output)';
            } else {
                $notes[] = '[PERHATIAN] Output komponen belum ditambahkan — kelengkapan foto diabaikan/tidak dapat dinilai (slot foto mengikuti komponen output)';
            }
        }

        if ($progressFull) {
            if (! $outputBelum) {
                $notes[] = 'Progress fisik sudah 100%';
            }
            // Jika output belum: sudah masuk blok kritis di atas
        } elseif ($progressRealisasi !== null && $progressRealisasi > 0) {
            $notes[] = 'Progress fisik '.rtrim(rtrim(number_format($progressRealisasi, 2, ',', '.'), '0'), ',').'%';
        } elseif ($fisikCount > 0) {
            $notes[] = 'Ada input progress fisik (estimasi % belum terisi)';
        } else {
            $notes[] = 'Progress fisik belum diinput';
        }

        if ($phoCompleted) {
            if (! $outputBelum) {
                if ($fotoBelum || $fotoPartial) {
                    $detailFoto = $fotoBelum
                        ? 'dokumentasi foto belum ada'
                        : (
                            $fotoRequired
                                ? "dokumentasi foto belum lengkap ({$fotoCount}/{$fotoRequired})"
                                : 'dokumentasi foto belum lengkap'
                        );
                    $notes[] = 'Sudah PHO tetapi '.$detailFoto;
                } else {
                    $notes[] = 'Sudah PHO dan dokumentasi foto lengkap';
                }

                if ($penerimaCount === 0) {
                    $notes[] = 'Sudah PHO tetapi penerima manfaat belum diinput';
                }
            } else {
                // Output kosong: foto diabaikan — jangan klaim "foto lengkap"
                if ($fotoCount > 0) {
                    $notes[] = "Ada {$fotoCount} foto, tetapi tanpa komponen output tidak dihitung kelengkapan";
                }
            }
        } else {
            if (! $outputBelum) {
                if ($fotoBelum) {
                    $notes[] = 'Dokumentasi foto belum ada';
                } elseif ($fotoPartial) {
                    $notes[] = $fotoRequired
                        ? "Dokumentasi foto belum lengkap ({$fotoCount}/{$fotoRequired})"
                        : 'Dokumentasi foto belum lengkap';
                }

                if ($penerimaCount === 0) {
                    $notes[] = 'Penerima manfaat belum diinput';
                }
            } elseif ($fotoCount > 0) {
                $notes[] = "Ada {$fotoCount} foto, tetapi tanpa komponen output tidak dihitung kelengkapan";
            }

            if ($outputBelum && $penerimaCount === 0 && ! $progressFull && ! $phoCompleted) {
                // Sudah disebut di blok kritis atau perhatian output; tambah penerima jika belum
                if (! str_contains(implode(' ', $notes), 'penerima')) {
                    $notes[] = 'Penerima manfaat belum diinput';
                }
            } elseif (! $outputBelum && $penerimaCount === 0) {
                // already handled above when not pho
            }
        }

        // Dedup while preserving order
        $notes = array_values(array_unique($notes));

        return implode('; ', $notes);
    }

    private function ensureKpiRoles(): void
    {
        foreach (self::KPI_ROLES as $roleName) {
            Role::firstOrCreate(['name' => $roleName]);
        }
    }

    /** @return list<string> */
    private function resolveRoleFilter(?string $peran): array
    {
        if ($peran === 'pengawas' || $peran === 'konsultan_pengawas') {
            return [$peran];
        }

        return self::KPI_ROLES;
    }

    /** @return list<string> */
    private function getUserKpiRoles(User $user): array
    {
        return array_values(array_intersect(
            $user->getRoleNames()->toArray(),
            self::KPI_ROLES
        ));
    }
}