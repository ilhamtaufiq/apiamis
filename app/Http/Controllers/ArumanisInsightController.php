<?php

namespace App\Http\Controllers;

use App\Models\Desa;
use App\Models\Kecamatan;
use App\Models\UnitSpam;
use App\Models\SpmSanitasi;
use App\Services\SpamPekerjaanIntegrationService;
use App\Services\SpmSanitasiCapaianService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class ArumanisInsightController extends Controller
{
    public function __construct(
        private readonly SpmSanitasiCapaianService $capaianService,
    ) {
    }

    public function index(Request $request): JsonResponse
    {
        $scope = $request->string('scope')->toString();
        $bidang = $request->string('bidang')->toString();
        $kecamatanId = $request->integer('kecamatan_id') ?: null;
        $desaId = $request->integer('desa_id') ?: null;

        // Resolve wilayah filter
        $wilayahFilter = fn ($query) => $query
            ->realWilayah()
            ->when($kecamatanId, fn ($q) => $q->where('kecamatan_id', $kecamatanId))
            ->when($desaId, fn ($q) => $q->where('id', $desaId));

        // ===== AIR MINUM (SPAM) =====
        $airMinum = $this->getAirMinumData(
            $scope,
            $wilayahFilter,
            $kecamatanId,
            $desaId,
            $bidang,
        );

        // ===== SANITASI =====
        $sanitasi = $this->getSanitasiData(
            $scope,
            $wilayahFilter,
            $kecamatanId,
            $desaId,
            $bidang,
        );

        // ===== PER-KECAMATAN DATA =====
        $kecamatanData = $this->getKecamatanData(
            $scope,
            $bidang,
            $wilayahFilter,
        );

        // ===== COMBINED SUMMARY =====
        $combined = $this->combineSummary($airMinum, $sanitasi, $bidang, $scope, $kecamatanId, $desaId);

        return response()->json([
            'success' => true,
            'data' => array_merge($combined, [
                'air_minum' => $airMinum,
                'sanitasi' => $sanitasi,
                'kecamatan_data' => $kecamatanData,
            ]),
        ]);
    }

    private function getAirMinumData(string $scope, $wilayahFilter, ?int $kecamatanId, ?int $desaId, string $bidang): array
    {
        if ($bidang !== 'all' && $bidang !== 'air_minum') {
            return ['enabled' => false];
        }

        $achievementQuery = \App\Models\SpamAchievement::query()
            ->where(function ($query) {
                $query->where('tahun', '<=', SpamPekerjaanIntegrationService::BASELINE_CAP_TAHUN)
                    ->orWhere('tahun', '>=', SpamPekerjaanIntegrationService::ACCUMULATION_START_TAHUN);
            });

        if ($desaId) {
            $achievementQuery->whereHas('unitSpam.desa', fn ($q) => $q->where('id', $desaId));
        } elseif ($kecamatanId) {
            $achievementQuery->whereHas('unitSpam.desa', fn ($q) => $q->where('kecamatan_id', $kecamatanId));
        }

        $totalSR = (int) $achievementQuery->sum('jumlah_sr');
        $totalKK = (int) $achievementQuery->sum('jumlah_kk');
        $totalJiwa = (int) $achievementQuery->sum('jumlah_jiwa');
        $bjpKK = (int) $achievementQuery->sum('jumlah_bjp_kk');

        $totalTarget = (int) $wilayahFilter((clone Desa::query()))->sum('target');
        $totalBJPKK = $totalKK + $bjpKK;
        $coveragePercentage = $totalTarget > 0
            ? round(($totalBJPKK / $totalTarget) * 100, 2)
            : 0;

        $totalUnits = UnitSpam::query()
            ->when($kecamatanId, fn ($q) => $q->whereHas('desa', fn ($dq) => $dq->where('kecamatan_id', $kecamatanId)))
            ->when($desaId, fn ($q) => $q->where('desa_id', $desaId))
            ->count();

        // NRW estimation: simple ratio dari pendapatan vs biaya operasional (fallback 15%)
        $nrd = $this->estimateNrd($kecamatanId, $desaId);

        $insight = $this->generateAirMinumInsight(
            $totalSR, $totalKK, $totalJiwa, $totalTarget,
            $coveragePercentage, $nrd, $totalUnits, $bidang
        );

        return [
            'enabled' => true,
            'total_sr' => $totalSR,
            'total_kk' => $totalKK,
            'total_jiwa' => $totalJiwa,
            'total_target' => $totalTarget,
            'total_bjp_kk' => $bjpKK,
            'coverage_percentage' => $coveragePercentage,
            'total_units' => $totalUnits,
            'nrd' => $nrd,
            'insight' => $insight,
        ];
    }

    private function getSanitasiData(string $scope, $wilayahFilter, ?int $kecamatanId, ?int $desaId, string $bidang): array
    {
        if ($bidang !== 'all' && $bidang !== 'sanitasi') {
            return ['enabled' => false];
        }

        $summary = $this->capaianService->summary($kecamatanId, null, null);

        $totalInfrastruktur = SpmSanitasi::query()
            ->whereHas('desa', fn ($dq) => $dq->realWilayah()
                ->when($kecamatanId, fn ($q) => $q->where('kecamatan_id', $kecamatanId))
                ->when($desaId, fn ($q) => $q->where('id', $desaId))
            )
            ->count();

        $insight = $this->generateSanitasiInsight(
            $summary, $totalInfrastruktur, $bidang
        );

        return [
            'enabled' => true,
            'total_infrastruktur' => $totalInfrastruktur,
            'total_pemanfaat_kk' => $summary['total_pemanfaat_kk'],
            'total_pemanfaat_jiwa' => $summary['total_pemanfaat_jiwa'],
            'total_penduduk' => $summary['total_penduduk'],
            'target_kk' => $summary['target_kk'],
            'coverage_percentage' => $summary['coverage_percentage'],
            'gap_kk' => $summary['gap_kk'],
            'gap_jiwa' => $summary['gap_jiwa'],
            'desa_with_infrastruktur' => $summary['desa_with_infrastruktur'],
            'desa_without_infrastruktur' => $summary['desa_without_infrastruktur'],
            'by_jenis' => $summary['by_jenis'],
            'insight' => $insight,
        ];
    }

    private function getKecamatanData(string $scope, string $bidang, $wilayahFilter): array
    {
        if ($scope !== 'all') {
            return [];
        }

        $kecamatanIds = Desa::query()
            ->realWilayah()
            ->when($scope === 'all', fn ($q) => $q)
            ->distinct()
            ->pluck('kecamatan_id')
            ->filter()
            ->all();

        if (empty($kecamatanIds)) {
            return [];
        }

        $kecamatanData = [];

        foreach (Kecamatan::query()->whereIn('id', $kecamatanIds)->orderBy('n_kec')->get() as $kec) {
            $kcFilter = fn ($q) => $q->where('kecamatan_id', $kec->id);
            $kcDesaFilter = fn ($q) => $q->where('kecamatan_id', $kec->id);

            if ($bidang === 'all' || $bidang === 'air_minum') {
                $am = $this->getAirMinumData($scope, $kcDesaFilter, $kec->id, null, $bidang);
            } else {
                $am = ['enabled' => false];
            }

            if ($bidang === 'all' || $bidang === 'sanitasi') {
                $sn = $this->getSanitasiData($scope, $kcDesaFilter, $kec->id, null, $bidang);
            } else {
                $sn = ['enabled' => false];
            }

            $combined = $this->combineSummary($am, $sn, $bidang, $scope);

            $highlight = array_merge(
                $am['insight']['highlight'] ?? [],
                $sn['insight']['highlight'] ?? [],
            );
            $detail = array_merge(
                $am['insight']['insight'] ?? [],
                $sn['insight']['insight'] ?? [],
            );
            $rekomendasi = array_merge(
                $am['insight']['rekomendasi'] ?? [],
                $sn['insight']['rekomendasi'] ?? [],
            );

            $coveragePercentage = ($am['enabled'] && $sn['enabled'])
                ? round(($am['coverage_percentage'] + $sn['coverage_percentage']) / 2, 2)
                : ($am['enabled'] ? ($am['coverage_percentage'] ?? 0) : ($sn['coverage_percentage'] ?? 0));

            $kecamatanData[] = [
                'kecamatan_id' => $kec->id,
                'kecamatan_name' => $kec->n_kec,
                'highlight' => array_map(fn ($i) => ['title' => $i['title'] ?? '', 'content' => $i['content'] ?? '', 'source' => $i['source'] ?? null], array_slice($highlight, 0, 3)),
                'insight' => array_map(fn ($i) => ['title' => $i['title'] ?? '', 'content' => $i['content'] ?? '', 'source' => $i['source'] ?? null], array_slice($detail, 0, 5)),
                'rekomendasi' => array_map(fn ($i) => ['title' => $i['title'] ?? '', 'content' => $i['content'] ?? '', 'source' => $i['source'] ?? null], array_slice($rekomendasi, 0, 3)),
                'coverage_percentage' => $coveragePercentage,
                'total_sr' => $am['enabled'] ? ($am['total_sr'] ?? 0) : null,
                'total_kk' => $am['enabled'] ? ($am['total_kk'] ?? 0) : null,
                'total_jiwa' => $am['enabled'] ? ($am['total_jiwa'] ?? 0) : null,
                'total_target' => $am['enabled'] ? ($am['total_target'] ?? 0) : null,
                'nrd' => $am['enabled'] ? ($am['nrd'] ?? null) : null,
                'desa_count' => Desa::query()->realWilayah()->where('kecamatan_id', $kec->id)->count(),
                'unit_count' => $am['enabled'] ? ($am['total_units'] ?? 0) : 0,
            ];
        }

        return $kecamatanData;
    }

    private function combineSummary(array $am, array $sn, string $bidang, string $scope, ?int $kecamatanId, ?int $desaId): array
    {
        $desaQuery = Desa::query()->realWilayah()
            ->when($kecamatanId, fn ($q) => $q->where('kecamatan_id', $kecamatanId))
            ->when($desaId, fn ($q) => $q->where('id', $desaId));
        $totalKecamatan = (int) $desaQuery->distinct()->count('kecamatan_id');
        $totalDesa = (int) $desaQuery->count();

        // Combined coverage: average of both domains when both enabled
        $amCoverage = $am['enabled'] ? ($am['coverage_percentage'] ?? 0) : null;
        $snCoverage = $sn['enabled'] ? ($sn['coverage_percentage'] ?? 0) : null;

        $combinedCoverage = null;
        if ($amCoverage !== null && $snCoverage !== null) {
            $combinedCoverage = round(($amCoverage + $snCoverage) / 2, 2);
        } elseif ($amCoverage !== null) {
            $combinedCoverage = $amCoverage;
        } elseif ($snCoverage !== null) {
            $combinedCoverage = $snCoverage;
        }

        $totalSR = $am['enabled'] ? ($am['total_sr'] ?? 0) : 0;
        $totalKK = $am['enabled'] ? ($am['total_kk'] ?? 0) : 0;
        $totalJiwa = ($am['enabled'] ? ($am['total_jiwa'] ?? 0) : 0) + ($sn['enabled'] ? ($sn['total_pemanfaat_jiwa'] ?? 0) : 0);
        $totalTarget = $am['enabled'] ? ($am['total_target'] ?? 0) : 0;

        $nrd = $am['enabled'] ? ($am['nrd'] ?? null) : null;

        // Combined insight
        $allHighlight = [];
        $allInsight = [];
        $allRekomendasi = [];

        if ($am['enabled'] && isset($am['insight'])) {
            $allHighlight = array_merge($allHighlight, $am['insight']['highlight'] ?? []);
            $allInsight = array_merge($allInsight, $am['insight']['insight'] ?? []);
            $allRekomendasi = array_merge($allRekomendasi, $am['insight']['rekomendasi'] ?? []);
        }
        if ($sn['enabled'] && isset($sn['insight'])) {
            $allHighlight = array_merge($allHighlight, $sn['insight']['highlight'] ?? []);
            $allInsight = array_merge($allInsight, $sn['insight']['insight'] ?? []);
            $allRekomendasi = array_merge($allRekomendasi, $sn['insight']['rekomendasi'] ?? []);
        }

        return [
            'total_kecamatan' => $totalKecamatan,
            'total_desa' => $totalDesa,
            'coverage_percentage' => $combinedCoverage ?? 0,
            'total_sr' => $totalSR,
            'total_kk' => $totalKK,
            'total_jiwa' => $totalJiwa,
            'total_target' => $totalTarget,
            'nrd' => $nrd,
            'highlight' => array_map(fn ($i) => ['title' => $i['title'] ?? '', 'content' => $i['content'] ?? '', 'source' => $i['source'] ?? null], $allHighlight),
            'insight' => array_map(fn ($i) => ['title' => $i['title'] ?? '', 'content' => $i['content'] ?? '', 'source' => $i['source'] ?? null], $allInsight),
            'rekomendasi' => array_map(fn ($i) => ['title' => $i['title'] ?? '', 'content' => $i['content'] ?? '', 'source' => $i['source'] ?? null], $allRekomendasi),
        ];
    }

    private function estimateNrd(?int $kecamatanId, ?int $desaId): ?float
    {
        // NRW estimation: query average of (pendapatan_bulan - biaya_operasional) / pendapatan_bulan
        // Fallback to 15% if no data
        $query = UnitSpam::query()
            ->whereNotNull('pendapatan_bulan')
            ->whereNotNull('biaya_operasional')
            ->where('pendapatan_bulan', '>', 0)
            ->selectRaw('AVG((biaya_operasional / pendapatan_bulan) * 100) as nrd');

        if ($desaId) {
            $query->where('desa_id', $desaId);
        } elseif ($kecamatanId) {
            $query->whereHas('desa', fn ($q) => $q->where('kecamatan_id', $kecamatanId));
        }

        $result = DB::selectOne($query->toSql(), $query->getBindings());

        if ($result && isset($result->nrd) && (float) $result->nrd > 0) {
            return round((float) $result->nrd, 2);
        }

        return null;
    }

    private function generateAirMinumInsight(int $totalSR, int $totalKK, int $totalJiwa, int $totalTarget, float $coverage, ?float $nrd, int $totalUnits, string $bidang): array
    {
        $highlight = [];
        $insight = [];
        $rekomendasi = [];
        $gapKk = max(0, $totalTarget - $totalKK);

        // Highlight
        if ($coverage >= 90) {
            $highlight[] = [
                'title' => 'Cakupan layanan tinggi',
                'content' => "Rasio sambungan terhadap target mencapai {$coverage}% ({$totalKK} KK dari {$totalTarget} KK target).",
                'source' => 'Data capaian SPAM',
            ];
        } elseif ($coverage >= 70) {
            $highlight[] = [
                'title' => 'Cakupan layanan sedang',
                'content' => "Rasio sambungan terhadap target {$coverage}%. Masih ada kesenjangan {$gapKk} KK yang belum tersambung.",
                'source' => 'Data capaian SPAM',
            ];
        } elseif ($coverage > 0) {
            $highlight[] = [
                'title' => 'Cakupan layanan rendah',
                'content' => "Rasio sambungan hanya {$coverage}%. Kesenjangan mencapai {$gapKk} KK dari {$totalTarget} KK target.",
                'source' => 'Data capaian SPAM',
            ];
        } else {
            $highlight[] = [
                'title' => 'Belum ada data capaian',
                'content' => "Tidak ada data capaian SPAM terdata. {$totalUnits} unit SPAM teridentifikasi di wilayah ini.",
                'source' => 'Unit SPAM master',
            ];
        }

        // Insight
        if ($totalUnits > 0 && $totalSR === 0) {
            $insight[] = [
                'title' => 'Unit SPAM belum memiliki capaian',
                'content' => "{$totalUnits} unit SPAM terdata namun belum memiliki input capaian (SR/KK/Jiwa). Perlu input data capaian manual atau integrasi pekerjaan.",
                'source' => 'Unit SPAM master',
            ];
        }

        if ($nrd !== null && $nrd > 25) {
            $insight[] = [
                'title' => 'NRD tinggi',
                'content' => "Non-Revenue Water (NRW) terestimasi di {$nrd}%, melebihi batas wajar 20-25%. Indicasi kebocoran pipa atau pembobolan sambungan.",
                'source' => 'Estimasi dari data keuangan unit',
            ];
        } elseif ($nrd !== null && $nrd <= 20) {
            $insight[] = [
                'title' => 'NRD dalam batas wajar',
                'content' => "NRD terestimasi di {$nrd}%, masih dalam batas efisien (<20%).",
                'source' => 'Estimasi dari data keuangan unit',
            ];
        }

        $insight[] = [
            'title' => 'Potensi pekerjaan terkait',
            'content' => 'Periksa tab "Integrasi" pada halaman SPAM untuk melihat paket pekerjaan yang bisa ditautkan ke unit SPAM.',
            'source' => 'Integrasi Pekerjaan-SPAM',
        ];

        // Rekomendasi
        if ($coverage < 70) {
            $rekomendasi[] = [
                'title' => 'Perlu percepatan sambungan baru',
                'content' => "Target tambahan {$gapKk} KK perlu direncanakan melalui program pembangunan SPAM baru atau perluasan jaringan.",
                'source' => 'Analisis gap cakupan',
            ];
        }

        if ($nrd !== null && $nrd > 20) {
            $rekomendasi[] = [
                'title' => 'Audit jaringan distribusi',
                'content' => "NRD {$nrd}% mengindikasikan kehilangan air signifikan. Rekomendasikan audit fisik jaringan dan zonal metering.",
                'source' => 'Analisis NRW',
            ];
        }

        $rekomendasi[] = [
            'title' => 'Pemanfaatan anggaran',
            'content' => 'Pemda dapat mengalokasikan anggaran air minum (DAK/HIBAH/APBD) melalui Disperkim Cianjur untuk pembangunan dan perluasan jaringan SPAM.',
            'source' => 'Best practice',
        ];

        return ['highlight' => $highlight, 'insight' => $insight, 'rekomendasi' => $rekomendasi];
    }

    private function generateSanitasiInsight(array $summary, int $totalInfrastruktur, string $bidang): array
    {
        $highlight = [];
        $insight = [];
        $rekomendasi = [];

        $coverage = $summary['coverage_percentage'];
        $desaWith = $summary['desa_with_infrastruktur'];
        $desaTotal = $summary['total_desa'];
        $desaWithout = $summary['desa_without_infrastruktur'];
        $gapJiwa = $summary['gap_jiwa'];

        if ($coverage >= 80) {
            $highlight[] = [
                'title' => 'Cakupan sanitasi baik',
                'content' => "Rasio pemanfaat terhadap penduduk mencapai {$coverage}%. {$desaWith} dari {$desaTotal} desa memiliki infrastruktur sanitasi.",
                'source' => 'Data SPM Sanitasi',
            ];
        } elseif ($coverage >= 50) {
            $highlight[] = [
                'title' => 'Cakupan sanitasi sedang',
                'content' => "Rasio pemanfaat terhadap penduduk {$coverage}%. Masih ada {$desaWithout} desa tanpa infrastruktur sanitasi terdata.",
                'source' => 'Data SPM Sanitasi',
            ];
        } elseif ($coverage > 0) {
            $highlight[] = [
                'title' => 'Cakupan sanitasi rendah',
                'content' => "Rasio pemanfaat hanya {$coverage}%. {$gapJiwa} jiwa belum terjangkau layanan sanitasi.",
                'source' => 'Data SPM Sanitasi',
            ];
        } else {
            $highlight[] = [
                'title' => 'Belum ada data sanitasi',
                'content' => "Tidak ada data infrastruktur sanitasi terdata di wilayah ini.",
                'source' => 'SPM Sanitasi master',
            ];
        }

        // Insight by jenis breakdown
        if (!empty($summary['by_jenis'])) {
            $jenisInsight = [];
            $jenisNames = [
                'spaldt' => 'SPALDT',
                'spalds' => 'SPALDS',
                'iplt' => 'IPLT',
                'mck_individu' => 'MCK Individu',
                'mck_komunal' => 'MCK Komunal',
            ];
            foreach ($summary['by_jenis'] as $jenis => $data) {
                if ($data['unit_count'] > 0) {
                    $name = $jenisNames[$jenis] ?? $jenis;
                    $jenisInsight[] = "{$name}: {$data['unit_count']} unit, {$data['pemanfaat_kk']} KK pemanfaat";
                }
            }
            if (!empty($jenisInsight)) {
                $insight[] = [
                    'title' => 'Komposisi infrastruktur',
                    'content' => implode(' | ', $jenisInsight),
                    'source' => 'SPM Sanitasi',
                ];
            }
        }

        if ($desaWithout > 0) {
            $insight[] = [
                'title' => 'Desa tanpa infrastruktur',
                'content' => "{$desaWithout} desa belum memiliki infrastruktur sanitasi terdata. Prioritaskan survey dan pendataan.",
                'source' => 'Analisis wilayah',
            ];
        }

        // Rekomendasi
        if ($gapJiwa > 0) {
            $rekomendasi[] = [
                'title' => 'Perluasan infrastruktur sanitasi',
                'content' => "{$gapJiwa} jiwa belum terjangkau. Evaluasi kebutuhan MCK komunal atau SPALD di desa target.",
                'source' => 'Analisis gap',
            ];
        }

        $rekomendasi[] = [
            'title' => 'Monitoring keberfungsian',
            'content' => 'Pastikan infrastruktur sanitasi yang sudah ada dalam status berfungsi dan terawat.',
            'source' => 'Best practice',
        ];

        return ['highlight' => $highlight, 'insight' => $insight, 'rekomendasi' => $rekomendasi];
    }
}
