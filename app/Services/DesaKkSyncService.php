<?php

namespace App\Services;

use App\Models\Desa;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use RuntimeException;

class DesaKkSyncService
{
    public const SOURCE_URL = 'https://opendata.cianjurkab.go.id/api/bigdata/dinas_kependudukan_dan_pencatatan_sipil_2/jumlah_kepemilikan_kartu_keluarga_kk_per_desa_di_ka_1';

    /**
     * Sync jumlah_kk from Cianjur open data.
     *
     * @return array{
     *     updated: int,
     *     unmatched: int,
     *     ambiguous: int,
     *     source_rows: int,
     *     tahun: int|null,
     *     semester: int|null,
     *     unmatched_samples: list<array{kecamatan: string, desa: string, jumlah_kk: int}>,
     *     ambiguous_samples: list<array{kecamatan: string, desa: string, jumlah_kk: int}>
     * }
     */
    public function sync(?int $tahun = null, ?int $semester = null): array
    {
        $rows = $this->fetchAllRows();
        if ($rows === []) {
            throw new RuntimeException('Tidak ada data KK dari open data Cianjur.');
        }

        [$tahun, $semester, $latestRows] = $this->selectPeriodRows($rows, $tahun, $semester);

        $lookup = $this->buildDesaLookup();
        $updated = 0;
        $unmatched = [];
        $ambiguous = [];

        foreach ($latestRows as $row) {
            $kecName = (string) ($row['bps_nama_kecamatan'] ?? $row['kemendagri_nama_kecamatan'] ?? '');
            $desaName = (string) ($row['bps_nama_desa_kelurahan'] ?? $row['kemendagri_nama_desa_kelurahan'] ?? '');
            $jumlahKk = (int) ($row['jumlah_kepemilikan_kk'] ?? 0);

            if ($kecName === '' || $desaName === '') {
                continue;
            }

            $key = $this->normalize($kecName).'|'.$this->normalize($desaName);
            $candidates = $lookup->get($key, collect());

            if ($candidates->isEmpty()) {
                $unmatched[] = [
                    'kecamatan' => $kecName,
                    'desa' => $desaName,
                    'jumlah_kk' => $jumlahKk,
                ];

                continue;
            }

            if ($candidates->count() > 1) {
                $ambiguous[] = [
                    'kecamatan' => $kecName,
                    'desa' => $desaName,
                    'jumlah_kk' => $jumlahKk,
                ];

                continue;
            }

            Desa::query()
                ->whereKey($candidates->first()->id)
                ->update(['jumlah_kk' => $jumlahKk]);

            $updated++;
        }

        return [
            'updated' => $updated,
            'unmatched' => count($unmatched),
            'ambiguous' => count($ambiguous),
            'source_rows' => count($latestRows),
            'tahun' => $tahun,
            'semester' => $semester,
            'unmatched_samples' => array_slice($unmatched, 0, 20),
            'ambiguous_samples' => array_slice($ambiguous, 0, 20),
        ];
    }

    /**
     * @return list<array<string, mixed>>
     */
    public function fetchAllRows(): array
    {
        $all = [];
        $page = 1;
        $totalPages = 1;
        $perPage = 1000;

        do {
            $response = Http::timeout(60)
                ->acceptJson()
                ->get(self::SOURCE_URL, [
                    'page' => $page,
                    'per_page' => $perPage,
                ]);

            if (! $response->successful()) {
                Log::warning('Gagal fetch open data KK Cianjur', [
                    'status' => $response->status(),
                    'page' => $page,
                    'body' => $response->body(),
                ]);

                throw new RuntimeException(
                    'Gagal mengambil data open data Cianjur (HTTP '.$response->status().').'
                );
            }

            $payload = $response->json();
            $rows = $payload['data'] ?? [];
            if (! is_array($rows)) {
                throw new RuntimeException('Format response open data tidak valid.');
            }

            foreach ($rows as $row) {
                if (is_array($row)) {
                    $all[] = $row;
                }
            }

            $pagination = $payload['pagination'] ?? [];
            $totalPages = max(1, (int) ($pagination['total_page'] ?? $page));
            $page++;
        } while ($page <= $totalPages);

        return $all;
    }

    /**
     * @param  list<array<string, mixed>>  $rows
     * @return array{0: int|null, 1: int|null, 2: list<array<string, mixed>>}
     */
    public function selectPeriodRows(array $rows, ?int $tahun = null, ?int $semester = null): array
    {
        if ($tahun === null || $semester === null) {
            $maxTahun = null;
            $maxSemester = null;

            foreach ($rows as $row) {
                $rowTahun = isset($row['tahun']) ? (int) $row['tahun'] : null;
                $rowSemester = isset($row['semester']) ? (int) $row['semester'] : null;
                if ($rowTahun === null || $rowSemester === null) {
                    continue;
                }

                if ($maxTahun === null
                    || $rowTahun > $maxTahun
                    || ($rowTahun === $maxTahun && $rowSemester > ($maxSemester ?? 0))
                ) {
                    $maxTahun = $rowTahun;
                    $maxSemester = $rowSemester;
                }
            }

            $tahun = $tahun ?? $maxTahun;
            $semester = $semester ?? $maxSemester;
        }

        $filtered = array_values(array_filter(
            $rows,
            fn (array $row): bool => (int) ($row['tahun'] ?? 0) === (int) $tahun
                && (int) ($row['semester'] ?? 0) === (int) $semester
        ));

        if ($filtered === []) {
            throw new RuntimeException(
                "Tidak ada data KK untuk tahun {$tahun} semester {$semester}."
            );
        }

        return [$tahun, $semester, $filtered];
    }

    private function buildDesaLookup(): Collection
    {
        return Desa::with('kecamatan')
            ->get()
            ->groupBy(
                fn (Desa $desa): string => $this->normalize((string) $desa->kecamatan?->n_kec)
                    .'|'.$this->normalize((string) $desa->n_desa)
            );
    }

    public function normalize(?string $value): string
    {
        if (! $value) {
            return '';
        }

        $value = str_ireplace(['kecamatan', 'desa', 'kelurahan'], '', $value);

        return strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
    }
}
