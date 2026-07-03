<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class KontrakAddendumResource extends JsonResource
{
    public function toArray($request)
    {
        return [
            'id' => $this->id,
            'kontrak_id' => $this->kontrak_id,
            'addendum_ke' => $this->addendum_ke,
            'nomor_addendum' => $this->nomor_addendum,
            'tanggal_addendum' => $this->tanggal_addendum?->format('Y-m-d'),
            'jenis_addendum' => $this->jenis_addendum,
            'alasan' => $this->alasan,
            'deskripsi_perubahan' => $this->deskripsi_perubahan,
            'nilai_kontrak_sebelum' => $this->nilai_kontrak_sebelum,
            'nilai_kontrak_sesudah' => $this->nilai_kontrak_sesudah,
            'tgl_selesai_sebelum' => $this->tgl_selesai_sebelum?->format('Y-m-d'),
            'tgl_selesai_sesudah' => $this->tgl_selesai_sesudah?->format('Y-m-d'),
            'status' => $this->status,
            'created_by' => $this->created_by,
            'approved_by' => $this->approved_by,
            'approved_at' => $this->approved_at?->toIso8601String(),
            'creator' => $this->whenLoaded('creator', fn () => $this->creator ? [
                'id' => $this->creator->id,
                'name' => $this->creator->name,
            ] : null),
            'approver' => $this->whenLoaded('approver', fn () => $this->approver ? [
                'id' => $this->approver->id,
                'name' => $this->approver->name,
            ] : null),
            'can_submit' => $this->status === 'draft' || $this->status === 'ditolak',
            'can_edit' => $this->status !== 'disetujui',
            'kontrak' => $this->whenLoaded('kontrak', fn () => [
                'id' => $this->kontrak->id,
                'spk' => $this->kontrak->spk,
                'kode_paket' => $this->kontrak->kode_paket,
                'nilai_kontrak' => $this->kontrak->nilai_kontrak,
                'tgl_selesai' => $this->kontrak->tgl_selesai?->format('Y-m-d'),
                'pekerjaan' => $this->kontrak->relationLoaded('pekerjaan') && $this->kontrak->pekerjaan ? [
                    'id' => $this->kontrak->pekerjaan->id,
                    'nama_paket' => $this->kontrak->pekerjaan->nama_paket,
                    'kode_rekening' => $this->kontrak->pekerjaan->kode_rekening,
                ] : null,
                'penyedia' => $this->kontrak->relationLoaded('penyedia') && $this->kontrak->penyedia ? [
                    'id' => $this->kontrak->penyedia->id,
                    'nama' => $this->kontrak->penyedia->nama,
                ] : null,
            ]),
            'items' => $this->whenLoaded('items', fn () => $this->items->map(fn ($item) => [
                'id' => $item->id,
                'nama_item' => $item->nama_item,
                'spesifikasi_sebelum' => $item->spesifikasi_sebelum,
                'spesifikasi_sesudah' => $item->spesifikasi_sesudah,
                'volume_sebelum' => $item->volume_sebelum,
                'volume_sesudah' => $item->volume_sesudah,
                'harga_sebelum' => $item->harga_sebelum,
                'harga_sesudah' => $item->harga_sesudah,
                'subtotal_sebelum' => $item->subtotal_sebelum,
                'subtotal_sesudah' => $item->subtotal_sesudah,
            ])),
            'attachments' => $this->getMedia('kontrak/addendum')->map(fn ($media) => [
                'id' => $media->id,
                'name' => $media->file_name,
                'url' => $media->getFullUrl(),
                'type' => $media->mime_type,
                'document_type' => $media->getCustomProperty('type'),
                'label' => $media->getCustomProperty('label'),
                'size' => $media->size,
            ]),
            'created_at' => $this->created_at?->toIso8601String(),
            'updated_at' => $this->updated_at?->toIso8601String(),
        ];
    }
}
