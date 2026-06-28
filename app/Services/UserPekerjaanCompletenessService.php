<?php

namespace App\Services;

use App\Models\Pekerjaan;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;

class UserPekerjaanCompletenessService
{
    public const GAP_FOTO = 'foto';

    public const GAP_PENERIMA = 'penerima';

    public const GAP_PROGRESS = 'progress';

    /**
     * @param  array<int, string>|null  $gapFilters
     * @return array{
     *     users: array<int, array{
     *         user_id: int,
     *         user_name: string,
     *         user_email: string,
     *         pekerjaan: array<int, array{
     *             pekerjaan_id: int,
     *             nama_paket: string,
     *             gaps: array<int, string>,
     *             gap_details: array<string, string>
     *         }>,
     *         gap_counts: array<string, int>
     *     }>,
     *     summary: array{
     *         total_users: int,
     *         total_pekerjaan_with_gaps: int,
     *         by_gap: array<string, int>
     *     }
     * }
     */
    public function analyze(?array $gapFilters = null, ?int $tahunAnggaran = null): array
    {
        $gapFilters = $this->normalizeGapFilters($gapFilters);
        $tahunAnggaran = $tahunAnggaran ?? (int) now()->year;

        $assignments = DB::table('user_pekerjaan')
            ->join('users', 'user_pekerjaan.user_id', '=', 'users.id')
            ->select(
                'user_pekerjaan.user_id',
                'user_pekerjaan.pekerjaan_id',
                'users.name as user_name',
                'users.email as user_email',
            )
            ->orderBy('users.name')
            ->get();

        if ($assignments->isEmpty()) {
            return [
                'users' => [],
                'summary' => [
                    'total_users' => 0,
                    'total_pekerjaan_with_gaps' => 0,
                    'by_gap' => array_fill_keys($gapFilters, 0),
                ],
            ];
        }

        $pekerjaanIds = $assignments->pluck('pekerjaan_id')->unique()->values()->all();

        $pekerjaanById = Pekerjaan::query()
            ->whereIn('id', $pekerjaanIds)
            ->with(['output', 'foto', 'progress', 'progressEstimasiHistory', 'kegiatan'])
            ->withCount('penerima')
            ->get()
            ->keyBy('id');

        $estimasiService = app(PekerjaanProgressEstimasiSummaryService::class);

        $users = [];
        $summaryByGap = array_fill_keys($gapFilters, 0);
        $totalPekerjaanWithGaps = 0;

        foreach ($assignments as $assignment) {
            $pekerjaan = $pekerjaanById->get($assignment->pekerjaan_id);
            if (! $pekerjaan) {
                continue;
            }

            if ($tahunAnggaran > 0) {
                $pekerjaanTahun = (int) ($pekerjaan->kegiatan?->tahun_anggaran ?? 0);
                if ($pekerjaanTahun > 0 && $pekerjaanTahun !== $tahunAnggaran) {
                    continue;
                }
            }

            $gaps = $this->detectGaps($pekerjaan, $estimasiService, $gapFilters);
            if ($gaps === []) {
                continue;
            }

            $userId = (int) $assignment->user_id;
            if (! isset($users[$userId])) {
                $users[$userId] = [
                    'user_id' => $userId,
                    'user_name' => $assignment->user_name,
                    'user_email' => $assignment->user_email,
                    'pekerjaan' => [],
                    'gap_counts' => array_fill_keys($gapFilters, 0),
                ];
            }

            foreach ($gaps as $gap) {
                $users[$userId]['gap_counts'][$gap] = ($users[$userId]['gap_counts'][$gap] ?? 0) + 1;
                $summaryByGap[$gap] = ($summaryByGap[$gap] ?? 0) + 1;
            }

            $users[$userId]['pekerjaan'][] = [
                'pekerjaan_id' => $pekerjaan->id,
                'nama_paket' => $pekerjaan->nama_paket,
                'gaps' => array_keys($gaps),
                'gap_details' => $gaps,
            ];

            $totalPekerjaanWithGaps++;
        }

        return [
            'users' => array_values($users),
            'summary' => [
                'total_users' => count($users),
                'total_pekerjaan_with_gaps' => $totalPekerjaanWithGaps,
                'by_gap' => $summaryByGap,
            ],
        ];
    }

    /**
     * @param  array<int, string>|null  $gapFilters
     * @return array<string>
     */
    private function normalizeGapFilters(?array $gapFilters): array
    {
        $allowed = [self::GAP_FOTO, self::GAP_PENERIMA, self::GAP_PROGRESS];
        $filters = $gapFilters === null || $gapFilters === []
            ? $allowed
            : array_values(array_intersect($allowed, $gapFilters));

        return $filters === [] ? $allowed : $filters;
    }

    /**
     * @param  array<int, string>  $gapFilters
     * @return array<string, string>
     */
    private function detectGaps(
        Pekerjaan $pekerjaan,
        PekerjaanProgressEstimasiSummaryService $estimasiService,
        array $gapFilters,
    ): array {
        $gaps = [];

        if (in_array(self::GAP_FOTO, $gapFilters, true) && $this->isMissingFoto($pekerjaan)) {
            $gaps[self::GAP_FOTO] = $this->describeFotoGap($pekerjaan);
        }

        if (in_array(self::GAP_PENERIMA, $gapFilters, true) && $this->isMissingPenerima($pekerjaan)) {
            $gaps[self::GAP_PENERIMA] = $this->describePenerimaGap($pekerjaan);
        }

        if (in_array(self::GAP_PROGRESS, $gapFilters, true) && $this->isMissingProgress($pekerjaan, $estimasiService)) {
            $gaps[self::GAP_PROGRESS] = 'Progress estimasi belum terinput';
        }

        return $gaps;
    }

    private function isMissingFoto(Pekerjaan $pekerjaan): bool
    {
        $metrics = $pekerjaan->resolveFotoMetrics();

        return ($metrics['foto_status'] ?? null) !== 'selesai';
    }

    private function describeFotoGap(Pekerjaan $pekerjaan): string
    {
        $metrics = $pekerjaan->resolveFotoMetrics();
        $status = $metrics['foto_status'] ?? 'belum_ada_foto';
        $count = (int) ($metrics['foto_count'] ?? 0);
        $required = $metrics['foto_required_count'];

        if ($status === 'belum_ada_foto') {
            return 'Belum ada foto dokumentasi';
        }

        if ($required !== null && $count < (int) $required) {
            return "Foto {$count}/{$required} slot";
        }

        return 'Foto dokumentasi belum lengkap';
    }

    private function isMissingPenerima(Pekerjaan $pekerjaan): bool
    {
        $requiredUnits = $this->requiredPenerimaUnits($pekerjaan);
        if ($requiredUnits <= 0) {
            return false;
        }

        $penerimaCount = (int) ($pekerjaan->penerima_count ?? 0);

        return $penerimaCount < $requiredUnits;
    }

    private function describePenerimaGap(Pekerjaan $pekerjaan): string
    {
        $requiredUnits = $this->requiredPenerimaUnits($pekerjaan);
        $penerimaCount = (int) ($pekerjaan->penerima_count ?? 0);

        if ($penerimaCount === 0) {
            return 'Daftar penerima masih kosong';
        }

        return "Penerima {$penerimaCount}/{$requiredUnits} unit";
    }

    private function requiredPenerimaUnits(Pekerjaan $pekerjaan): int
    {
        if (! $pekerjaan->relationLoaded('output') || $pekerjaan->output->isEmpty()) {
            return 0;
        }

        $required = 0;

        foreach ($pekerjaan->output as $output) {
            if ($output->penerima_is_optional) {
                continue;
            }

            $required += max(1, (int) ceil((float) ($output->volume ?? 0)));
        }

        return $required;
    }

    private function isMissingProgress(
        Pekerjaan $pekerjaan,
        PekerjaanProgressEstimasiSummaryService $estimasiService,
    ): bool {
        $progressTotal = $this->calculateProgressTotal($pekerjaan);
        if ($progressTotal > 0) {
            return false;
        }

        $tahunAnggaran = (int) ($pekerjaan->kegiatan?->tahun_anggaran ?? now()->year);
        $histories = $pekerjaan->relationLoaded('progressEstimasiHistory')
            ? $pekerjaan->progressEstimasiHistory
            : collect();

        $estimasi = $estimasiService->summarize($histories instanceof Collection ? $histories : collect($histories), $tahunAnggaran);
        $hasEstimasiInput = $estimasi['fisik_realisasi'] !== null
            || $estimasi['fisik_rencana'] !== null
            || $estimasi['keuangan_realisasi'] !== null
            || $estimasi['keuangan_rencana'] !== null;

        return ! $hasEstimasiInput;
    }

    private function calculateProgressTotal(Pekerjaan $pekerjaan): float
    {
        if (! $pekerjaan->relationLoaded('progress') || ! $pekerjaan->progress) {
            return 0;
        }

        $items = $pekerjaan->progress->content['items'] ?? [];
        if ($items === []) {
            return 0;
        }

        $maxReportedWeek = 0;
        foreach ($items as $item) {
            foreach ($item['weekly_data'] ?? [] as $minggu => $data) {
                if (isset($data['realisasi']) && $data['realisasi'] !== null) {
                    $maxReportedWeek = max($maxReportedWeek, (int) $minggu);
                }
            }
        }

        $progressTotal = 0.0;

        foreach ($items as $item) {
            $bobot = (float) ($item['bobot'] ?? 0);
            $targetVolume = (float) ($item['target_volume'] ?? 0);
            $itemTotalReal = 0.0;

            foreach ($item['weekly_data'] ?? [] as $minggu => $data) {
                $realisasi = $data['realisasi'] ?? null;
                if ($realisasi !== null) {
                    $itemTotalReal += (float) $realisasi;
                }
            }

            $progressPercent = $targetVolume > 0 ? ($itemTotalReal / $targetVolume) * 100 : 0;
            $progressTotal += ($progressPercent * $bobot) / 100;
        }

        return round($progressTotal, 2);
    }

    /**
     * @param  array<int, array{
     *     user_id: int,
     *     user_name: string,
     *     pekerjaan: array<int, array{nama_paket: string, gaps: array<int, string>, gap_details: array<string, string>}>
     * }>  $users
     */
    public function buildReminderMessage(array $userRow, ?string $customPrefix = null): string
    {
        $lines = [];
        $gapLabels = [
            self::GAP_FOTO => 'foto',
            self::GAP_PENERIMA => 'penerima',
            self::GAP_PROGRESS => 'progress',
        ];

        foreach ($userRow['pekerjaan'] as $pekerjaan) {
            $gapNames = array_map(
                fn (string $gap) => $gapLabels[$gap] ?? $gap,
                $pekerjaan['gaps'],
            );
            $lines[] = '• ' . $pekerjaan['nama_paket'] . ' — belum lengkap: ' . implode(', ', $gapNames);
        }

        $prefix = $customPrefix !== null && trim($customPrefix) !== ''
            ? trim($customPrefix) . "\n\n"
            : "Halo {$userRow['user_name']},\n\nBeberapa pekerjaan yang ditugaskan kepada Anda masih belum lengkap:\n\n";

        return $prefix . implode("\n", $lines) . "\n\nSilakan lengkapi data di aplikasi pengawasan.";
    }
}