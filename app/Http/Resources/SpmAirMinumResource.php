<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class SpmAirMinumResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        $jenisJaringan = strtoupper($request->string('jenis_jaringan')->toString());
        $selectedLayanan = match ($jenisJaringan) {
            'JP' => (int) $this->jp_jiwa_terlayani,
            'BJP' => (int) $this->bjp_jiwa_terlayani,
            default => (int) $this->total_jiwa_terlayani,
        };
        $target = $this->target_total_jiwa !== null ? (int) $this->target_total_jiwa : null;
        $belumTerlayani = $target !== null ? max($target - $selectedLayanan, 0) : null;
        $persentase = $target ? round(($selectedLayanan / $target) * 100, 2) : null;
        $status = $target ? ($selectedLayanan >= $target ? 'terpenuhi' : 'belum_terpenuhi') : 'data_kurang';

        return [
            'id' => $this->id,
            'kecamatan_id' => $this->kecamatan_id,
            'desa_id' => $this->desa_id,
            'kecamatan' => $this->kecamatan?->n_kec,
            'desa' => $this->desa?->n_desa,
            'target_total_jiwa' => $this->target_total_jiwa,
            'jp_jiwa_terlayani' => $jenisJaringan === 'BJP' ? 0 : $this->jp_jiwa_terlayani,
            'bjp_jiwa_terlayani' => $jenisJaringan === 'JP' ? 0 : $this->bjp_jiwa_terlayani,
            'total_jiwa_terlayani' => $selectedLayanan,
            'belum_terlayani' => $belumTerlayani,
            'persentase_layanan' => $persentase,
            'status_spm' => $status,
            'tahun_data' => $this->tahun_data,
            'last_consolidated_at' => $this->last_consolidated_at?->toIso8601String(),
            'sources' => $this->whenLoaded('sources'),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
