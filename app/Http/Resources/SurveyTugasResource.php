<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class SurveyTugasResource extends JsonResource
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

        $statusLabel = [
            'ditugaskan' => 'Ditugaskan',
            'dikerjakan' => 'Dikerjakan',
            'selesai' => 'Selesai',
        ];

        $surveysCount = $this->whenCounted('surveys_count', fn () => (int) $this->surveys_count);

        return [
            'id' => $this->id,
            'judul' => $this->judul,
            'tahun_anggaran' => $this->tahun_anggaran !== null ? (int) $this->tahun_anggaran : null,
            'jenis' => $this->jenis,
            'jenis_label' => $this->jenis ? ($jenisLabel[$this->jenis] ?? $this->jenis) : null,
            'pekerjaan' => $this->whenLoaded('pekerjaan', function () {
                return $this->pekerjaan ? [
                    'id' => $this->pekerjaan->id,
                    'nama_paket' => $this->pekerjaan->nama_paket,
                ] : null;
            }),
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
            'assignee' => $this->whenLoaded('assignee', function () {
                return $this->assignee ? [
                    'id' => $this->assignee->id,
                    'name' => $this->assignee->name,
                ] : null;
            }),
            'creator' => $this->whenLoaded('creator', function () {
                return $this->creator ? [
                    'id' => $this->creator->id,
                    'name' => $this->creator->name,
                ] : null;
            }),
            'status' => $this->status,
            'status_label' => $statusLabel[$this->status] ?? $this->status,
            'batas_waktu' => $this->batas_waktu?->format('Y-m-d'),
            'catatan_admin' => $this->catatan_admin,
            'surveys_count' => $surveysCount,
            'sudah_disurvey' => $this->when(
                $this->relationLoaded('surveys') || isset($this->surveys_count),
                fn () => ((int) ($this->surveys_count ?? $this->surveys?->count() ?? 0)) > 0
            ),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
