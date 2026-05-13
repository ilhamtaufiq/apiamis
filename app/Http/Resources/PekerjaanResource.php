<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use App\Http\Resources\KecamatanResource;
use App\Http\Resources\DesaResource;
use App\Http\Resources\KegiatanResource;
use App\Http\Resources\TagResource;

class PekerjaanResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     */
    public function toArray(Request $request): array
    {
        $user = auth()->user();
        $sources = [];
        
        if ($user && !$user->hasRole('admin')) {
            static $assignedIds = null;
            static $roleKegiatanIds = null;

            if ($assignedIds === null) {
                $assignedIds = \Illuminate\Support\Facades\DB::table('user_pekerjaan')
                    ->where('user_id', $user->id)
                    ->pluck('pekerjaan_id')
                    ->toArray();
            }

            if ($roleKegiatanIds === null) {
                $userRoleIds = $user->roles()->pluck('id')->toArray();
                $roleKegiatanIds = \App\Models\KegiatanRole::whereIn('role_id', $userRoleIds)
                    ->pluck('kegiatan_id')
                    ->toArray();
            }

            if (in_array($this->id, $assignedIds)) $sources[] = 'manual';
            if (in_array($this->kegiatan_id, $roleKegiatanIds)) $sources[] = 'role';
            
            if ($user->nip) {
                if ($this->pengawas_id && $this->pengawas && $this->pengawas->nip === $user->nip) $sources[] = 'pengawas';
                if ($this->pendamping_id && $this->pendamping && $this->pendamping->nip === $user->nip) $sources[] = 'pendamping';
            }
        }

        $progressTotal = 0;
        if ($this->relationLoaded('progress') && $this->progress) {
            $content = $this->progress->content ?? [];
            $items = $content['items'] ?? [];
            
            foreach ($items as $item) {
                $bobot = (float) ($item['bobot'] ?? 0);
                $weeklyData = $item['weekly_data'] ?? [];
                $itemTotalReal = 0;
                
                foreach ($weeklyData as $minggu => $data) {
                    $realisasi = $data['realisasi'] ?? 0;
                    if ($realisasi !== null) {
                        $itemTotalReal += $realisasi;
                    }
                }
                
                $targetVolume = (float) ($item['target_volume'] ?? 0);
                $progressPercent = $targetVolume > 0 
                    ? ($itemTotalReal / $targetVolume) * 100 
                    : 0;
                $weightedProgress = ($progressPercent * $bobot) / 100;
                $progressTotal += $weightedProgress;
            }
        }

        return [
            'id' => $this->id,
            'kode_rekening' => $this->kode_rekening,
            'nama_paket' => $this->nama_paket,
            'pagu' => $this->pagu,
            'progress_total' => round($progressTotal, 2),
            'kecamatan_id' => $this->kecamatan_id,
            'desa_id' => $this->desa_id,
            'kegiatan_id' => $this->kegiatan_id,
            'pengawas_id' => $this->pengawas_id,
            'pendamping_id' => $this->pendamping_id,
            'assignment_sources' => $sources,
            'kecamatan' => new KecamatanResource($this->whenLoaded('kecamatan')),
            'desa' => new DesaResource($this->whenLoaded('desa')),
            'kegiatan' => new KegiatanResource($this->whenLoaded('kegiatan')),
            'pengawas' => new PengawasResource($this->whenLoaded('pengawas')),
            'pendamping' => new PengawasResource($this->whenLoaded('pendamping')),
            'tags' => TagResource::collection($this->whenLoaded('tags')),
            'draft' => new DraftPekerjaanResource($this->whenLoaded('draft')),
            'penerima_count' => $this->penerima_count,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
