<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use App\Http\Resources\KecamatanResource;
use App\Http\Resources\DesaResource;
use App\Http\Resources\KegiatanResource;

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
            // Check manual assignment
            $isManual = \Illuminate\Support\Facades\DB::table('user_pekerjaan')
                ->where('user_id', $user->id)
                ->where('pekerjaan_id', $this->id)
                ->exists();
            if ($isManual) $sources[] = 'manual';
            
            // Check role assignment
            $userRoleIds = $user->roles()->pluck('id')->toArray();
            $isRole = \App\Models\KegiatanRole::whereIn('role_id', $userRoleIds)
                ->where('kegiatan_id', $this->kegiatan_id)
                ->exists();
            if ($isRole) $sources[] = 'role';
        }

        return [
            'id' => $this->id,
            'kode_rekening' => $this->kode_rekening,
            'nama_paket' => $this->nama_paket,
            'pagu' => $this->pagu,
            'kecamatan_id' => $this->kecamatan_id,
            'desa_id' => $this->desa_id,
            'kegiatan_id' => $this->kegiatan_id,
            'assignment_sources' => $sources,
            'kecamatan' => new KecamatanResource($this->whenLoaded('kecamatan')),
            'desa' => new DesaResource($this->whenLoaded('desa')),
            'kegiatan' => new KegiatanResource($this->whenLoaded('kegiatan')),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
