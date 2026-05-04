<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;
use App\Http\Resources\KecamatanResource;
use App\Http\Resources\DesaResource;
use App\Http\Resources\KegiatanResource;
use App\Http\Resources\FotoResource;
use App\Http\Resources\BerkasResource;
use App\Http\Resources\KontrakResource;
use App\Http\Resources\OutputResource;
use App\Http\Resources\PenerimaResource;
use App\Http\Resources\TagResource;
use App\Http\Resources\ProgressResource;


class PekerjaanDetailResource extends JsonResource
{
    /**
     * Transform the resource into an array (dengan semua relasi).
     */
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'kode_rekening' => $this->kode_rekening,
            'nama_paket' => $this->nama_paket,
            'pagu' => $this->pagu,
            'kecamatan_id' => $this->kecamatan_id,
            'desa_id' => $this->desa_id,
            'kegiatan_id' => $this->kegiatan_id,
            'pengawas_id' => $this->pengawas_id,
            'pendamping_id' => $this->pendamping_id,
            'assignment_sources' => (function() {
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
                }
                return $sources;
            })(),
            'kecamatan' => new KecamatanResource($this->whenLoaded('kecamatan')),
            'desa' => new DesaResource($this->whenLoaded('desa')),
            'kegiatan' => new KegiatanResource($this->whenLoaded('kegiatan')),
            'pengawas' => new PengawasResource($this->whenLoaded('pengawas')),
            'pendamping' => new PengawasResource($this->whenLoaded('pendamping')),

            // Tambahkan relasi baru foto dan berkas
            'foto' => FotoResource::collection($this->whenLoaded('foto')),
            'berkas' => BerkasResource::collection($this->whenLoaded('berkas')),
            'kontrak' => KontrakResource::collection($this->whenLoaded('kontrak')),
            'output' => OutputResource::collection($this->whenLoaded('output')),
            'penerima' => PenerimaResource::collection($this->whenLoaded('penerima')),
            'tags' => TagResource::collection($this->whenLoaded('tags')),
            'progress' => new ProgressResource($this->whenLoaded('progress')),


            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
