<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PekerjaanResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     */
    public function toArray(Request $request): array
    {
        $user = auth()->user();
        $sources = [];

        if ($user && ! $user->hasRole('admin')) {
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

            if (in_array($this->id, $assignedIds)) {
                $sources[] = 'manual';
            }
            if (in_array($this->kegiatan_id, $roleKegiatanIds)) {
                $sources[] = 'role';
            }

            if ($user->nip) {
                if ($this->pengawas_id && $this->pengawas && $this->pengawas->nip === $user->nip) {
                    $sources[] = 'pengawas';
                }
                if ($this->pendamping_id && $this->pendamping && $this->pendamping->nip === $user->nip) {
                    $sources[] = 'pendamping';
                }
            }
        }

        $progressTotal = 0;
        $progressRencana = 0;
        $deviasi = 0;

        if ($this->relationLoaded('progress') && $this->progress) {
            $content = $this->progress->content ?? [];
            $items = $content['items'] ?? [];

            // 1. Temukan minggu terakhir yang ada laporan realisasinya
            $maxReportedWeek = 0;
            foreach ($items as $item) {
                $weeklyData = $item['weekly_data'] ?? [];
                foreach ($weeklyData as $minggu => $data) {
                    if (isset($data['realisasi']) && $data['realisasi'] !== null) {
                        $maxReportedWeek = max($maxReportedWeek, (int) $minggu);
                    }
                }
            }

            // 2. Hitung progres fisik dan rencana (rencana hanya dihitung sampai maxReportedWeek)
            foreach ($items as $item) {
                $bobot = (float) ($item['bobot'] ?? 0);
                $weeklyData = $item['weekly_data'] ?? [];
                $itemTotalReal = 0;
                $itemTotalRencana = 0;

                foreach ($weeklyData as $minggu => $data) {
                    $realisasi = $data['realisasi'] ?? null;
                    if ($realisasi !== null) {
                        $itemTotalReal += $realisasi;
                    }

                    if ((int) $minggu <= $maxReportedWeek) {
                        $rencana = $data['rencana'] ?? 0;
                        if ($rencana !== null) {
                            $itemTotalRencana += $rencana;
                        }
                    }
                }

                $targetVolume = (float) ($item['target_volume'] ?? 0);

                $progressPercent = $targetVolume > 0
                    ? ($itemTotalReal / $targetVolume) * 100
                    : 0;
                $weightedProgress = ($progressPercent * $bobot) / 100;
                $progressTotal += $weightedProgress;

                $rencanaPercent = $targetVolume > 0
                    ? ($itemTotalRencana / $targetVolume) * 100
                    : 0;
                $weightedRencana = ($rencanaPercent * $bobot) / 100;
                $progressRencana += $weightedRencana;
            }

            $deviasi = $progressTotal - $progressRencana;
        }

        return [
            'id' => $this->id,
            'kode_rekening' => $this->kode_rekening,
            'nama_paket' => $this->nama_paket,
            'pagu' => $this->pagu,
            'progress_total' => round($progressTotal, 2),
            'deviasi' => round($deviasi, 2),
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
            'kontrak' => $this->whenLoaded('kontrak', fn () => $this->kontrak->map(fn ($kontrak) => [
                'id' => $kontrak->id,
                'spk' => $kontrak->spk,
                'kode_paket' => $kontrak->kode_paket,
                'tgl_spmk' => $kontrak->tgl_spmk?->format('Y-m-d'),
                'tgl_selesai' => $kontrak->tgl_selesai?->format('Y-m-d'),
                'nilai_kontrak' => $kontrak->nilai_kontrak,
                'penyedia' => $kontrak->relationLoaded('penyedia') && $kontrak->penyedia ? [
                    'id' => $kontrak->penyedia->id,
                    'nama' => $kontrak->penyedia->nama,
                ] : null,
                'addendums' => $kontrak->relationLoaded('addendums')
                    ? $kontrak->addendums->map(fn ($addendum) => [
                        'id' => $addendum->id,
                        'addendum_ke' => $addendum->addendum_ke,
                        'nomor_addendum' => $addendum->nomor_addendum,
                        'tanggal_addendum' => $addendum->tanggal_addendum?->format('Y-m-d'),
                        'status' => $addendum->status,
                    ])->values()
                    : [],
            ])->values()),
            'penerima_count' => $this->penerima_count,
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
