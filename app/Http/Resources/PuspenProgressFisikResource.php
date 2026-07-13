<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\Schema;

class PuspenProgressFisikResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        $rencana = $this->progress_fisik?->rencana;
        $realisasi = $this->progress_fisik?->realisasi;
        $outputs = $this->resolveOutputs($request);
        $hasOutputs = count($outputs) > 0;
        $subKegiatan = collect([$this->kegiatan?->nama_sub_kegiatan])
            ->merge(
                $this->pekerjaans
                    ->pluck('kegiatan.nama_sub_kegiatan')
            )
            ->filter()
            ->unique()
            ->values()
            ->implode(', ');

        $pagu = $this->resolvePagu();
        $nilaiKontrak = $this->resolveNilaiKontrak();
        $sisaKontrak = max(0, $pagu - $nilaiKontrak);
        $retensi = round($nilaiKontrak * 0.05, 2);

        $payload = [
            'kontrak_id' => $this->id,
            'kode_paket' => $this->kode_paket,
            'nama_paket' => $this->resolveNamaPaket(),
            'sub_kegiatan' => $subKegiatan,
            'tahun_anggaran' => (int) $request->integer('tahun', now()->year),
            'pagu' => $pagu,
            'nilai_kontrak' => $nilaiKontrak,
            'sisa_kontrak' => $sisaKontrak,
            'retensi' => $retensi,
            'rencana' => $rencana,
            'realisasi' => $realisasi,
            'deviasi' => $realisasi !== null && $rencana !== null
                ? round($realisasi - $rencana, 2)
                : null,
            'updated_at' => $this->progress_fisik?->updated_at?->toISOString(),
            'outputs' => $outputs,
            'has_outputs' => $hasOutputs,
            'output_notice' => $hasOutputs
                ? null
                : 'Output pekerjaan belum diinput di master data. Lengkapi komponen output pada menu Pekerjaan terlebih dahulu.',
        ];

        if (Schema::hasColumn('puspen_progress_fisik', 'pho_completed')) {
            $payload['pho_completed'] = (bool) ($this->progress_fisik?->pho_completed ?? false);
        }

        return $payload;
    }

    private function resolveNamaPaket(): string
    {
        $names = $this->pekerjaans
            ->pluck('nama_paket')
            ->filter()
            ->values();

        if ($this->pekerjaan?->nama_paket) {
            $names->prepend($this->pekerjaan->nama_paket);
        }

        return $names->unique()->values()->implode(', ');
    }

    private function resolvePagu(): float
    {
        $pekerjaans = collect();

        if ($this->relationLoaded('pekerjaans')) {
            $pekerjaans = $pekerjaans->merge($this->pekerjaans);
        }

        if ($this->relationLoaded('pekerjaan') && $this->pekerjaan) {
            $pekerjaans->push($this->pekerjaan);
        }

        $paguSum = (float) $pekerjaans
            ->filter(fn ($p) => $p && $p->id)
            ->unique('id')
            ->sum(fn ($p) => (float) ($p->pagu ?? 0));

        if ($paguSum > 0) {
            return $paguSum;
        }

        // Fallback: pagu sub kegiatan jika pagu pekerjaan kosong
        if ($this->relationLoaded('kegiatan') && $this->kegiatan?->pagu !== null) {
            return (float) $this->kegiatan->pagu;
        }

        return 0.0;
    }

    private function resolveNilaiKontrak(): float
    {
        if (method_exists($this->resource, 'nilaiKontrakBerjalan')) {
            return (float) ($this->resource->nilaiKontrakBerjalan() ?? $this->nilai_kontrak ?? 0);
        }

        return (float) ($this->nilai_kontrak ?? 0);
    }

    /**
     * @return array<int, array<string, mixed>>
     */
    private function resolveOutputs(Request $request): array
    {
        $savedRealisasi = $this->relationLoaded('progress_fisik_outputs')
            ? $this->progress_fisik_outputs->keyBy('output_id')
            : collect();

        $outputs = $this->collectLinkedOutputs();

        return $outputs
            ->filter(fn ($output) => $output && $output->id)
            ->unique('id')
            ->sortBy(fn ($output) => mb_strtolower(trim((string) ($output->komponen ?? ''))))
            ->values()
            ->map(function ($output) use ($savedRealisasi) {
                $saved = $savedRealisasi->get($output->id);

                return [
                    'output_id' => $output->id,
                    'pekerjaan_id' => $output->pekerjaan_id,
                    'komponen' => $output->komponen,
                    'volume' => (float) ($output->volume ?? 0),
                    'satuan' => $output->satuan,
                    'realisasi' => $saved?->realisasi,
                    'updated_at' => $saved?->updated_at?->toISOString(),
                ];
            })
            ->all();
    }

    private function collectLinkedOutputs(): Collection
    {
        $outputs = collect();

        if ($this->relationLoaded('pekerjaan') && $this->pekerjaan?->relationLoaded('output')) {
            $outputs = $outputs->merge($this->pekerjaan->output);
        }

        if ($this->relationLoaded('pekerjaans')) {
            foreach ($this->pekerjaans as $pekerjaan) {
                if ($pekerjaan->relationLoaded('output')) {
                    $outputs = $outputs->merge($pekerjaan->output);
                }
            }
        }

        return $outputs
            ->filter(fn ($output) => $output && $output->id)
            ->values();
    }
}