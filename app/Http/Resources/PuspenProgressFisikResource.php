<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PuspenProgressFisikResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        $rencana = $this->progress_fisik?->rencana;
        $realisasi = $this->progress_fisik?->realisasi;

        return [
            'kontrak_id' => $this->id,
            'kode_paket' => $this->kode_paket,
            'nama_paket' => $this->pekerjaans
                ->pluck('nama_paket')
                ->filter()
                ->values()
                ->implode(', '),
            'tahun_anggaran' => (int) $request->integer('tahun', now()->year),
            'rencana' => $rencana,
            'realisasi' => $realisasi,
            'deviasi' => $realisasi !== null && $rencana !== null
                ? round($realisasi - $rencana, 2)
                : null,
        ];
    }
}
