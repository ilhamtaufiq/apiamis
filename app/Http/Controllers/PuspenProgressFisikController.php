<?php

namespace App\Http\Controllers;

use App\Http\Resources\PuspenProgressFisikResource;
use App\Models\AppSetting;
use App\Models\Kontrak;
use App\Models\PuspenProgressFisik;
use Illuminate\Http\Request;

class PuspenProgressFisikController extends Controller
{
    public function index(Request $request)
    {
        $validated = $request->validate([
            'tahun' => 'nullable|integer|min:2000|max:2100',
            'search' => 'nullable|string|max:100',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:1000',
        ]);

        $tahun = (int) ($validated['tahun'] ?? now()->year);
        return $this->progressResponse($request, $tahun, $validated['search'] ?? null, (int) ($validated['per_page'] ?? 15));
    }

    public function publicIndex(Request $request)
    {
        if (AppSetting::getValue('puspen_progress_fisik_public', '0') !== '1') {
            return response()->json(['message' => 'Halaman progress fisik Puspen sedang dikunci'], 403);
        }

        $validated = $request->validate([
            'search' => 'nullable|string|max:100',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:1000',
        ]);

        $tahun = (int) (AppSetting::getValue('tahun_anggaran') ?: now()->year);
        return $this->progressResponse($request, $tahun, $validated['search'] ?? null, (int) ($validated['per_page'] ?? 15));
    }

    private function progressResponse(Request $request, int $tahun, ?string $search, int $perPage)
    {
        $request->merge(['tahun' => $tahun]);

        $query = Kontrak::query()
            ->with([
                'pekerjaans:id,nama_paket',
                'progress_fisik' => fn ($q) => $q->where('tahun_anggaran', $tahun),
            ])
            ->where(function ($q) use ($tahun) {
                $q->whereHas('kegiatan', fn ($k) => $k->where('tahun_anggaran', $tahun))
                    ->orWhereHas('pekerjaans.kegiatan', fn ($k) => $k->where('tahun_anggaran', $tahun));
            })
            ->orderBy('kode_paket')
            ->orderBy('id');

        if ($search) {
            $query->where(function ($q) use ($search) {
                $q->where('kode_paket', 'like', "%{$search}%")
                    ->orWhereHas('pekerjaans', fn ($p) => $p->where('nama_paket', 'like', "%{$search}%"));
            });
        }

        return PuspenProgressFisikResource::collection($query->paginate($perPage));
    }

    public function bulkUpdate(Request $request)
    {
        $validated = $this->validateProgressPayload($request, true);

        foreach ($validated['items'] as $item) {
            PuspenProgressFisik::updateOrCreate(
                [
                    'kontrak_id' => $item['kontrak_id'],
                    'tahun_anggaran' => $validated['tahun'],
                ],
                [
                    'rencana' => $item['rencana'] ?? null,
                    'realisasi' => $item['realisasi'] ?? null,
                ]
            );
        }

        return response()->json(['message' => 'Progress fisik berhasil disimpan']);
    }

    public function publicBulkUpdate(Request $request)
    {
        if (AppSetting::getValue('puspen_progress_fisik_public', '0') !== '1') {
            return response()->json(['message' => 'Halaman progress fisik Puspen sedang dikunci'], 403);
        }

        $validated = $this->validateProgressPayload($request, false);

        $tahun = (int) (AppSetting::getValue('tahun_anggaran') ?: now()->year);

        foreach ($validated['items'] as $item) {
            PuspenProgressFisik::updateOrCreate(
                [
                    'kontrak_id' => $item['kontrak_id'],
                    'tahun_anggaran' => $tahun,
                ],
                [
                    'rencana' => $item['rencana'] ?? null,
                    'realisasi' => $item['realisasi'] ?? null,
                ]
            );
        }

        return response()->json(['message' => 'Progress fisik berhasil disimpan']);
    }

    private function validateProgressPayload(Request $request, bool $needsYear): array
    {
        $rules = [
            'items' => 'required|array',
            'items.*.kontrak_id' => 'required|integer|exists:tbl_kontrak,id',
            'items.*.rencana' => ['nullable', function ($attribute, $value, $fail) {
                $this->validateDecimalInput($attribute, $value, $fail);
            }],
            'items.*.realisasi' => ['nullable', function ($attribute, $value, $fail) {
                $this->validateDecimalInput($attribute, $value, $fail);
            }],
        ];

        if ($needsYear) {
            $rules['tahun'] = 'required|integer|min:2000|max:2100';
        }

        return $request->validate($rules);
    }

    private function validateDecimalInput(string $attribute, mixed $value, \Closure $fail): void
    {
        if ($value === null || $value === '') {
            return;
        }

        $stringValue = trim((string) $value);

        if (preg_match('/^\d+([.,]\d{1,2})?$/', $stringValue) !== 1) {
            $fail("Field {$attribute} harus berupa angka desimal dengan maksimal 2 angka di belakang koma.");
            return;
        }

        $normalized = str_replace(',', '.', $stringValue);
        if (! is_numeric($normalized)) {
            $fail("Field {$attribute} harus berupa angka desimal.");
            return;
        }

        $numeric = (float) $normalized;
        if ($numeric < 0 || $numeric > 100) {
            $fail("Field {$attribute} harus berada antara 0 dan 100.");
        }
    }
}
