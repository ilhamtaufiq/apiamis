<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class SurveyLokasiResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     *
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        $jenisLabel = [
            'spam_perpipaan' => 'SPAM Perpipaan',
            'spam_pengeboran' => 'SPAM Pengeboran',
            'mck_individu' => 'MCK Individu',
            'mck_komunal' => 'MCK Komunal',
        ];

        return [
            'id' => $this->id,
            'jenis' => $this->jenis,
            'jenis_label' => $jenisLabel[$this->jenis] ?? $this->jenis,
            'nama_lokasi' => $this->nama_lokasi,
            'kecamatan' => $this->whenLoaded('kecamatan', function () {
                return $this->kecamatan ? [
                    'id' => $this->kecamatan->id,
                    'nama' => $this->kecamatan->n_kec,
                ] : null;
            }),
            'desa' => $this->whenLoaded('desa', function () {
                return $this->desa ? [
                    'id' => $this->desa->id,
                    'nama' => $this->desa->n_desa,
                ] : null;
            }),
            'kecamatan_id' => $this->kecamatan_id,
            'desa_id' => $this->desa_id,
            'alamat' => $this->alamat,
            'latitude' => $this->latitude !== null ? (float) $this->latitude : null,
            'longitude' => $this->longitude !== null ? (float) $this->longitude : null,
            'detail' => $this->detail,
            'status' => $this->status,
            'catatan_verifikasi' => $this->catatan_verifikasi,
            'verified_by' => $this->whenLoaded('verifier', function () {
                return $this->verifier ? [
                    'id' => $this->verifier->id,
                    'name' => $this->verifier->name,
                ] : null;
            }),
            'verified_at' => $this->verified_at?->toIso8601String(),
            'surveyor' => $this->whenLoaded('user', function () {
                return $this->user ? [
                    'id' => $this->user->id,
                    'name' => $this->user->name,
                ] : null;
            }),
            'tugas' => $this->whenLoaded('tugas', function () {
                return $this->tugas ? [
                    'id' => $this->tugas->id,
                    'judul' => $this->tugas->judul,
                    'status' => $this->tugas->status,
                ] : null;
            }),
            'foto' => $this->when(
                $this->relationLoaded('media') || method_exists($this->resource, 'getMedia'),
                function () {
                    return $this->getMedia('foto')->map(function ($media) {
                        return [
                            'id' => $media->id,
                            'url' => $media->getUrl(),
                            'name' => $media->file_name,
                            'size' => $media->size,
                            'kategori' => $media->getCustomProperty('kategori'),
                        ];
                    })->values()->all();
                }
            ),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
