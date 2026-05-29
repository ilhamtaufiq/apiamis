<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class KontrakDetailResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'kode_rup' => $this->kode_rup,
            'kode_paket' => $this->kode_paket,
            'nomor_penawaran' => $this->nomor_penawaran,
            'tanggal_penawaran' => $this->tanggal_penawaran?->format('Y-m-d'),
            'nilai_kontrak' => $this->nilai_kontrak,
            'tgl_sppbj' => $this->tgl_sppbj?->format('Y-m-d'),
            'tgl_spk' => $this->tgl_spk?->format('Y-m-d'),
            'tgl_spmk' => $this->tgl_spmk?->format('Y-m-d'),
            'tgl_selesai' => $this->tgl_selesai?->format('Y-m-d'),
            'sppbj' => $this->sppbj,
            'spk' => $this->spk,
            'spmk' => $this->spmk,
            'id_kegiatan' => $this->id_kegiatan,
            'id_pekerjaan' => $this->id_pekerjaan,
            'id_penyedia' => $this->id_penyedia,
            'nilai_kontrak_berjalan' => $this->nilaiKontrakBerjalan(),
            'tgl_selesai_berjalan' => $this->tglSelesaiBerjalan()?->format('Y-m-d'),
            'kegiatan' => new KegiatanResource($this->whenLoaded('kegiatan')),
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'penyedia' => new PenyediaResource($this->whenLoaded('penyedia')),
            'is_checklist_complete' => $this->pekerjaan ? $this->pekerjaan->isChecklistComplete() : false,
            'latest_approved_addendum' => new KontrakAddendumResource($this->whenLoaded('latestApprovedAddendum')),
            'addendums' => KontrakAddendumResource::collection($this->whenLoaded('addendums')),
            'contract_versions' => $this->whenLoaded('addendums', fn () => collect([
                [
                    'type' => 'utama',
                    'label' => 'Kontrak Utama',
                    'nomor' => $this->spk ?: $this->kode_paket,
                    'tanggal' => $this->tgl_spk?->format('Y-m-d'),
                    'nilai_kontrak' => $this->nilai_kontrak,
                    'tgl_selesai' => $this->tgl_selesai?->format('Y-m-d'),
                    'status' => 'utama',
                ],
            ])->merge($this->addendums->map(fn ($addendum) => [
                'type' => 'addendum',
                'id' => $addendum->id,
                'label' => 'Addendum ke-'.$addendum->addendum_ke,
                'addendum_ke' => $addendum->addendum_ke,
                'nomor' => $addendum->nomor_addendum,
                'tanggal' => $addendum->tanggal_addendum?->format('Y-m-d'),
                'nilai_kontrak' => $addendum->nilai_kontrak_sesudah,
                'tgl_selesai' => $addendum->tgl_selesai_sesudah?->format('Y-m-d'),
                'status' => $addendum->status,
            ]))->values()),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
