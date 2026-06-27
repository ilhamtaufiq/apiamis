<?php

namespace App\Http\Controllers;

use App\Exports\SpmSanitasiExport;
use App\Models\SpmSanitasi;
use App\Services\SpmSanitasiCapaianService;
use App\Services\SpmSanitasiImportService;
use App\Services\SpmSanitasiPekerjaanIntegrationService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Maatwebsite\Excel\Facades\Excel;

class SpmSanitasiController extends Controller
{
    public function __construct(
        private readonly SpmSanitasiCapaianService $capaianService,
        private readonly SpmSanitasiPekerjaanIntegrationService $integrationService,
    ) {
    }
    public function index(Request $request): JsonResponse
    {
        $query = SpmSanitasi::with(['desa.kecamatan']);

        if ($request->filled('jenis')) {
            $query->where('jenis', $request->jenis);
        }

        if ($request->filled('kecamatan_id')) {
            $query->whereHas('desa', fn ($q) => $q->where('kecamatan_id', $request->kecamatan_id));
        }

        if ($request->filled('desa_id')) {
            $query->where('desa_id', $request->desa_id);
        }

        if ($request->filled('search')) {
            $search = '%'.$request->search.'%';
            $query->where(function ($q) use ($search) {
                $q->where('nama_infrastruktur', 'like', $search)
                    ->orWhere('alamat_lengkap', 'like', $search)
                    ->orWhere('pengelola', 'like', $search)
                    ->orWhereHas('desa', fn ($dq) => $dq->where('n_desa', 'like', $search));
            });
        }

        $items = $query->orderByDesc('id')->paginate($request->integer('per_page', 15));

        return response()->json([
            'success' => true,
            'data' => $items->items(),
            'meta' => [
                'current_page' => $items->currentPage(),
                'last_page' => $items->lastPage(),
                'per_page' => $items->perPage(),
                'total' => $items->total(),
            ],
        ]);
    }

    public function stats(Request $request): JsonResponse
    {
        $kecamatanId = $request->integer('kecamatan_id') ?: null;
        $jenis = $request->filled('jenis') ? $request->string('jenis')->toString() : null;

        $counts = SpmSanitasi::query()
            ->when($kecamatanId, fn ($q) => $q->whereHas('desa', fn ($dq) => $dq->where('kecamatan_id', $kecamatanId)))
            ->when($jenis, fn ($q) => $q->where('jenis', $jenis))
            ->selectRaw('jenis, COUNT(*) as total')
            ->groupBy('jenis')
            ->pluck('total', 'jenis');

        $baseQuery = SpmSanitasi::query()
            ->when($kecamatanId, fn ($q) => $q->whereHas('desa', fn ($dq) => $dq->where('kecamatan_id', $kecamatanId)))
            ->when($jenis, fn ($q) => $q->where('jenis', $jenis));

        $berfungsi = (clone $baseQuery)->where('status_keberfungsian', 'Berfungsi')->count();
        $totalPemanfaat = (int) (clone $baseQuery)->sum('jumlah_pemanfaat_kk');
        $totalInvestasi = (float) (clone $baseQuery)->sum('pembiayaan_total');
        $capaian = $this->capaianService->summary($kecamatanId, $jenis);

        return response()->json([
            'success' => true,
            'data' => array_merge([
                'spaldt_count' => (int) ($counts['spaldt'] ?? 0),
                'spalds_count' => (int) ($counts['spalds'] ?? 0),
                'iplt_count' => (int) ($counts['iplt'] ?? 0),
                'mck_individu_count' => (int) ($counts['mck_individu'] ?? 0),
                'mck_komunal_count' => (int) ($counts['mck_komunal'] ?? 0),
                'total_count' => (clone $baseQuery)->count(),
                'berfungsi_count' => $berfungsi,
                'total_pemanfaat_kk' => $totalPemanfaat,
                'total_investasi' => $totalInvestasi,
            ], $capaian),
        ]);
    }

    public function capaian(Request $request): JsonResponse
    {
        $request->validate([
            'jenis' => 'nullable|in:spaldt,spalds,iplt,mck_individu,mck_komunal',
            'sort' => 'nullable|in:coverage_percentage,jumlah_penduduk,pemanfaat_kk,n_desa',
            'direction' => 'nullable|in:asc,desc',
        ]);

        $kecamatanId = $request->integer('kecamatan_id') ?: null;
        $jenis = $request->filled('jenis') ? $request->string('jenis')->toString() : null;

        $summary = $this->capaianService->summary($kecamatanId, $jenis);
        $desaPaginator = $this->capaianService->paginateDesa(
            $kecamatanId,
            $jenis,
            $request->filled('search') ? $request->string('search')->toString() : null,
            $request->integer('page', 1),
            $request->integer('per_page', 15),
            $request->string('sort', 'coverage_percentage')->toString(),
            $request->string('direction', 'asc')->toString(),
        );

        return response()->json([
            'success' => true,
            'summary' => $summary,
            'data' => $desaPaginator->items(),
            'meta' => [
                'current_page' => $desaPaginator->currentPage(),
                'last_page' => $desaPaginator->lastPage(),
                'per_page' => $desaPaginator->perPage(),
                'total' => $desaPaginator->total(),
            ],
        ]);
    }

    public function integration(Request $request): JsonResponse
    {
        $request->validate([
            'sync_status' => 'nullable|in:matched,partial,no_infrastruktur,no_pekerjaan',
            'output_type' => 'nullable|in:mck,mck_individu,mck_komunal,tangki_septik,tangki_septik_individu,tangki_septik_komunal,ipal',
        ]);

        $result = $this->integrationService->paginateIntegration(
            $request->filled('tahun') ? $request->string('tahun')->toString() : null,
            $request->integer('kecamatan_id') ?: null,
            $request->integer('desa_id') ?: null,
            $request->filled('search') ? $request->string('search')->toString() : null,
            $request->filled('sync_status') ? $request->string('sync_status')->toString() : null,
            $request->filled('output_type') ? $request->string('output_type')->toString() : null,
            $request->integer('per_page', 15),
            $request->integer('page', 1),
        );

        return response()->json([
            'success' => true,
            'data' => $result['data'],
            'meta' => $result['meta'],
            'summary' => $result['summary'],
        ]);
    }

    public function integrationByDesa(Request $request, int $desaId): JsonResponse
    {
        $request->validate([
            'output_type' => 'nullable|in:mck,mck_individu,mck_komunal,tangki_septik,tangki_septik_individu,tangki_septik_komunal,ipal',
        ]);

        $desa = \App\Models\Desa::query()->findOrFail($desaId);

        return response()->json([
            'success' => true,
            'data' => $this->integrationService->buildDesaIntegrationRow(
                $desa,
                $request->filled('tahun') ? $request->string('tahun')->toString() : null,
                $request->filled('output_type') ? $request->string('output_type')->toString() : null,
            ),
        ]);
    }

    public function mckPekerjaan(Request $request): JsonResponse
    {
        $request->validate([
            'mck_type' => 'nullable|in:mck,mck_individu,mck_komunal,tangki_septik,tangki_septik_individu,tangki_septik_komunal,ipal',
            'output_type' => 'nullable|in:mck,mck_individu,mck_komunal,tangki_septik,tangki_septik_individu,tangki_septik_komunal,ipal',
        ]);

        $outputType = $request->filled('output_type')
            ? $request->string('output_type')->toString()
            : ($request->filled('mck_type') ? $request->string('mck_type')->toString() : null);

        $result = $this->integrationService->paginateSanitasiPekerjaan(
            $request->filled('tahun') ? $request->string('tahun')->toString() : null,
            $request->integer('kecamatan_id') ?: null,
            $request->integer('desa_id') ?: null,
            $request->filled('search') ? $request->string('search')->toString() : null,
            $outputType,
            $request->integer('spm_sanitasi_id') ?: null,
            $request->boolean('unlinked_only') ?: null,
            $request->integer('per_page', 15),
            $request->integer('page', 1),
        );

        return response()->json([
            'success' => true,
            'data' => $result['data'],
            'meta' => $result['meta'],
        ]);
    }

    public function attachPekerjaan(Request $request, SpmSanitasi $spmSanitasi): JsonResponse
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'output_id' => 'nullable|exists:tbl_output,id',
        ]);

        try {
            $this->integrationService->attachPekerjaan(
                $spmSanitasi,
                (int) $validated['pekerjaan_id'],
                isset($validated['output_id']) ? (int) $validated['output_id'] : null,
            );
        } catch (\InvalidArgumentException $e) {
            return response()->json(['success' => false, 'message' => $e->getMessage()], 422);
        }

        $spmSanitasi->refresh()->load(['desa.kecamatan', 'pekerjaan.kegiatan', 'pekerjaan.output', 'pekerjaan.kontrak']);

        return response()->json([
            'success' => true,
            'message' => 'Pekerjaan berhasil ditautkan. Tahun konstruksi dan total pembiayaan diperbarui dari data pekerjaan (kontrak, atau pagu jika kontrak kosong).',
            'data' => $spmSanitasi,
        ]);
    }

    public function detachPekerjaan(SpmSanitasi $spmSanitasi, int $pekerjaanId): JsonResponse
    {
        $this->integrationService->detachPekerjaan($spmSanitasi, $pekerjaanId);

        $spmSanitasi->refresh();

        return response()->json([
            'success' => true,
            'message' => 'Tautan pekerjaan berhasil dihapus. Total pembiayaan disesuaikan ulang dari pekerjaan yang masih tertaut.',
            'data' => $spmSanitasi,
        ]);
    }

    public function show(SpmSanitasi $spmSanitasi): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => $spmSanitasi->load([
                'desa.kecamatan',
                'pekerjaan.kegiatan',
                'pekerjaan.output',
                'pekerjaan.desa',
            ]),
        ]);
    }

    public function store(Request $request): JsonResponse
    {
        $validated = $this->validatePayload($request);
        $item = SpmSanitasi::create($validated);

        return response()->json([
            'success' => true,
            'data' => $item->load('desa.kecamatan'),
            'message' => 'Data SPM Sanitasi berhasil ditambahkan',
        ], 201);
    }

    public function update(Request $request, SpmSanitasi $spmSanitasi): JsonResponse
    {
        $validated = $this->validatePayload($request, false);
        $spmSanitasi->update($validated);

        return response()->json([
            'success' => true,
            'data' => $spmSanitasi->load('desa.kecamatan'),
            'message' => 'Data SPM Sanitasi berhasil diperbarui',
        ]);
    }

    public function destroy(SpmSanitasi $spmSanitasi): JsonResponse
    {
        $spmSanitasi->delete();

        return response()->json([
            'success' => true,
            'message' => 'Data SPM Sanitasi berhasil dihapus',
        ]);
    }

    public function import(Request $request): JsonResponse
    {
        $request->validate([
            'file' => 'required|file|mimes:xlsx,xls,csv|max:20480',
            'replace' => 'sometimes|boolean',
        ]);

        try {
            $service = new SpmSanitasiImportService();
            $service->import(
                $request->file('file')->getRealPath(),
                $request->boolean('replace')
            );

            return response()->json([
                'success' => true,
                'message' => 'Data SPM Sanitasi berhasil diimport',
                'imported_rows' => $service->importedRows,
                'skipped_rows' => $service->skippedRows,
                'errors' => $service->errors,
            ]);
        } catch (\Throwable $e) {
            return response()->json([
                'success' => false,
                'message' => 'Gagal mengimport data: '.$e->getMessage(),
            ], 500);
        }
    }

    public function export(Request $request)
    {
        return Excel::download(
            new SpmSanitasiExport(
                $request->integer('kecamatan_id') ?: null,
                $request->integer('desa_id') ?: null,
                $request->string('search')->toString() ?: null,
            ),
            'data_spm_sanitasi.xlsx'
        );
    }

    public function downloadTemplate()
    {
        return Excel::download(new SpmSanitasiExport(), 'template_spm_sanitasi.xlsx');
    }

    private function validatePayload(Request $request, bool $requireJenis = true): array
    {
        $rules = [
            'jenis' => ($requireJenis ? 'required' : 'sometimes').'|in:spaldt,spalds,iplt,mck_individu,mck_komunal',
            'desa_id' => 'nullable|exists:tbl_desa,id',
            'skala_pelayanan' => 'nullable|string|max:255',
            'nama_infrastruktur' => 'required|string|max:500',
            'latitude' => 'nullable|numeric',
            'longitude' => 'nullable|numeric',
            'alamat_lengkap' => 'nullable|string',
            'jumlah_pemanfaat_kk' => 'nullable|integer|min:0',
            'jumlah_pemanfaat_jiwa' => 'nullable|integer|min:0',
            'tahun_konstruksi' => 'nullable|integer|min:1900|max:2100',
            'pembiayaan_apbn' => 'nullable|numeric|min:0',
            'pembiayaan_apbd' => 'nullable|numeric|min:0',
            'pembiayaan_dak' => 'nullable|numeric|min:0',
            'pembiayaan_hibah' => 'nullable|numeric|min:0',
            'pembiayaan_csr' => 'nullable|numeric|min:0',
            'pembiayaan_lain' => 'nullable|numeric|min:0',
            'pembiayaan_total' => 'nullable|numeric|min:0',
            'status_keberfungsian' => 'nullable|string|max:100',
            'kualitas_keberfungsian' => 'nullable|string|max:100',
            'pengelola' => 'nullable|string|max:255',
            'kapasitas_desain' => 'nullable|numeric|min:0',
            'kapasitas_terpakai' => 'nullable|numeric|min:0',
            'kapasitas_tidak_terpakai' => 'nullable|numeric|min:0',
            'jenis_pengolahan' => 'nullable|string|max:255',
            'peta_cakupan' => 'nullable|string|max:100',
            'status_lahan' => 'nullable|string|max:100',
            'luas_lahan_ha' => 'nullable|string|max:50',
            'opsi_teknologi' => 'nullable|string|max:255',
            'jumlah_stasiun_pompa' => 'nullable|string|max:50',
            'biaya_operasional' => 'nullable|numeric|min:0',
            'jenis_pengelola' => 'nullable|string|max:255',
            'sistem_pengolahan' => 'nullable|string|max:255',
            'truk_tinja_unit' => 'nullable|integer|min:0',
            'kapasitas_truk_m3' => 'nullable|numeric|min:0',
            'jumlah_ritasi' => 'nullable|integer|min:0',
            'jarak_maksimal_pelayanan_km' => 'nullable|numeric|min:0',
            'alokasi_biaya_operasional' => 'nullable|numeric|min:0',
        ];

        return $request->validate($rules);
    }
}