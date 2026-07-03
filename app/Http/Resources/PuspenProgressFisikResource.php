<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use Illuminate\Support\Collection;

class PuspenProgressFisikResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        $rencana = $this->progress_fisik?->rencana;
        $realisasi = $this->progress_fisik?->realisasi;
        $outputs = $this->resolveOutputs($request);
        $subKegiatan = collect([$this->kegiatan?->nama_sub_kegiatan])
            ->merge(
                $this->pekerjaans
                    ->pluck('kegiatan.nama_sub_kegiatan')
            )
            ->filter()
            ->unique()
            ->values()
            ->implode(', ');

        return [
            'kontrak_id' => $this->id,
            'kode_paket' => $this->kode_paket,
            'nama_paket' => $this->pekerjaans
                ->pluck('nama_paket')
                ->filter()
                ->values()
                ->implode(', '),
            'sub_kegiatan' => $subKegiatan,
            'tahun_anggaran' => (int) $request->integer('tahun', now()->year),
            'rencana' => $rencana,
            'realisasi' => $realisasi,
            'deviasi' => $realisasi !== null && $rencana !== null
                ? round($realisasi - $rencana, 2)
                : null,
            'updated_at' => $this->progress_fisik?->updated_at?->toISOString(),
            'outputs' => $outputs,
        ];
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
            ->unique('id')
            ->sortBy('komponen')
            ->values()
            ->map(function ($output) use ($savedRealisasi) {
                $saved = $savedRealisasi->get($output->id);

                return [
                    'output_id' => $output->id,
                    'pekerjaan_id' => $output->pekerjaan_id,
                    'komponen' => $output->komponen,
                    'volume' => (float) $output->volume,
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

        return $outputs;
    }
}