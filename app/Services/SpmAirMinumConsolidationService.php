<?php

namespace App\Services;

use App\Models\Desa;
use App\Models\SpamKelembagaanRaw;
use App\Models\SpamTerbangunRaw;
use App\Models\SpamWilayahMatch;
use App\Models\SpmAirMinum;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;

class SpmAirMinumConsolidationService
{
    public function consolidate(): array
    {
        return DB::transaction(function () {
            $lookup = $this->buildDesaLookup();
            $matchStats = $this->refreshMatches($lookup);
            $spmStats = $this->refreshSpm();

            return [
                ...$matchStats,
                ...$spmStats,
            ];
        });
    }

    private function buildDesaLookup(): array
    {
        $lookup = [];

        Desa::with('kecamatan')->get()->each(function (Desa $desa) use (&$lookup) {
            $kecamatanKey = $this->normalize((string) $desa->kecamatan?->n_kec);
            $desaKey = $this->normalize((string) $desa->n_desa);

            if (! $kecamatanKey || ! $desaKey) {
                return;
            }

            $lookup["{$kecamatanKey}|{$desaKey}"][] = $desa;
        });

        return $lookup;
    }

    private function refreshMatches(array $lookup): array
    {
        $matched = 0;
        $unmatched = 0;
        $ambiguous = 0;

        foreach (SpamTerbangunRaw::cursor() as $record) {
            $status = $this->matchRecord(
                sourceType: 'terbangun_raw',
                sourceId: $record->id,
                kecamatanRaw: $record->kecamatan,
                desaRaw: $record->desa_kelurahan,
                lookup: $lookup,
            );
            ${$status}++;
        }

        foreach (SpamKelembagaanRaw::cursor() as $record) {
            $status = $this->matchRecord(
                sourceType: 'kelembagaan_raw',
                sourceId: $record->id,
                kecamatanRaw: $record->kecamatan,
                desaRaw: $record->desa_kelurahan,
                lookup: $lookup,
            );
            ${$status}++;
        }

        return [
            'matched' => $matched,
            'unmatched' => $unmatched,
            'ambiguous' => $ambiguous,
        ];
    }

    private function matchRecord(string $sourceType, int $sourceId, ?string $kecamatanRaw, ?string $desaRaw, array $lookup): string
    {
        $key = $this->normalize($kecamatanRaw).'|'.$this->normalize($desaRaw);
        $candidates = $lookup[$key] ?? [];
        $status = 'unmatched';
        $score = 0;
        $desa = null;
        $notes = null;

        if (count($candidates) === 1) {
            $status = 'matched';
            $score = 100;
            $desa = $candidates[0];
        } elseif (count($candidates) > 1) {
            $status = 'ambiguous';
            $score = 70;
            $notes = 'Lebih dari satu kandidat desa master.';
        }

        SpamWilayahMatch::updateOrCreate(
            ['source_type' => $sourceType, 'source_id' => $sourceId],
            [
                'kecamatan_raw' => $kecamatanRaw,
                'desa_raw' => $desaRaw,
                'kecamatan_id' => $desa?->kecamatan_id,
                'desa_id' => $desa?->id,
                'match_status' => $status,
                'match_score' => $score,
                'notes' => $notes,
            ],
        );

        return $status;
    }

    private function refreshSpm(): array
    {
        $created = 0;
        $now = now();
        $desaList = Desa::with('kecamatan')->get();

        foreach ($desaList as $desa) {
            $kelembagaanIds = SpamWilayahMatch::query()
                ->where('source_type', 'kelembagaan_raw')
                ->where('desa_id', $desa->id)
                ->where('match_status', 'matched')
                ->pluck('source_id');

            $terbangunIds = SpamWilayahMatch::query()
                ->where('source_type', 'terbangun_raw')
                ->where('desa_id', $desa->id)
                ->where('match_status', 'matched')
                ->pluck('source_id');

            $kelembagaan = SpamKelembagaanRaw::whereIn('id', $kelembagaanIds)->get();
            $terbangun = SpamTerbangunRaw::whereIn('id', $terbangunIds)->get();

            $target = $desa->jumlah_penduduk ?: $terbangun->max('jumlah_penduduk');
            $jp = (int) $kelembagaan->where('jenis_jaringan', 'JP')->sum('jiwa_terlayani');
            $bjp = (int) $kelembagaan->where('jenis_jaringan', 'BJP')->sum('jiwa_terlayani');
            $total = $jp + $bjp;
            $belum = $target !== null ? max((int) $target - $total, 0) : null;
            $persen = $target ? round(($total / (int) $target) * 100, 2) : null;
            $status = $target ? ($total >= (int) $target ? 'terpenuhi' : 'belum_terpenuhi') : 'data_kurang';
            $year = $this->latestYear($kelembagaan, $terbangun);

            $spm = SpmAirMinum::updateOrCreate(
                ['desa_id' => $desa->id],
                [
                    'kecamatan_id' => $desa->kecamatan_id,
                    'target_total_jiwa' => $target,
                    'jp_jiwa_terlayani' => $jp,
                    'bjp_jiwa_terlayani' => $bjp,
                    'total_jiwa_terlayani' => $total,
                    'belum_terlayani' => $belum,
                    'persentase_layanan' => $persen,
                    'status_spm' => $status,
                    'tahun_data' => $year,
                    'last_consolidated_at' => $now,
                ],
            );

            $spm->sources()->delete();
            $this->insertSources($spm, $kelembagaan, $terbangun);
            $created++;
        }

        return ['consolidated' => $created];
    }

    private function insertSources(SpmAirMinum $spm, Collection $kelembagaan, Collection $terbangun): void
    {
        foreach ($kelembagaan as $record) {
            $spm->sources()->create([
                'source_type' => 'kelembagaan_raw',
                'source_id' => $record->id,
                'jenis_jaringan' => $record->jenis_jaringan,
                'sr_unit' => $record->sr_unit,
                'kk_terlayani' => $record->kk_terlayani,
                'jiwa_terlayani' => $record->jiwa_terlayani,
                'nama_pengelola' => $record->nama_pengelola,
                'tahun_pembangunan_raw' => $record->tahun_pembangunan_raw,
                'sumber_dana_raw' => $record->sumber_dana_raw,
            ]);
        }

        foreach ($terbangun as $record) {
            $anggaran = collect([
                $record->nilai_dak_apbn_rp,
                $record->nilai_apbd_rp,
                $record->nilai_banprov_rp,
            ])->filter(fn ($value) => $value !== null)->sum(fn ($value) => (float) $value);

            $spm->sources()->create([
                'source_type' => 'terbangun_raw',
                'source_id' => $record->id,
                'sr_unit' => $record->sr_unit,
                'jiwa_terlayani' => $record->penduduk_terlayani,
                'kondisi' => $record->kondisi_normalized,
                'nama_pengelola' => $record->nama_pengelola,
                'tahun_pembangunan_raw' => $record->tahun_pembangunan_raw,
                'sumber_dana_raw' => $record->sumber_dana_raw,
                'anggaran_rp' => $anggaran > 0 ? $anggaran : null,
            ]);
        }
    }

    private function latestYear(Collection $kelembagaan, Collection $terbangun): ?int
    {
        return collect([
            $kelembagaan->max('tahun_pembangunan_akhir'),
            $terbangun->max('tahun_pembangunan_akhir'),
        ])->filter()->max();
    }

    private function normalize(?string $value): ?string
    {
        if (! $value) {
            return null;
        }

        $value = str_ireplace(['kecamatan', 'desa', 'kelurahan'], '', $value);

        return strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
    }
}
