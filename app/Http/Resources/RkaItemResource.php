<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class RkaItemResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'id' => $this->id,
            'rka_document_id' => $this->rka_document_id,
            'kode_rekening' => $this->kode_rekening,
            'tipe' => $this->tipe,
            'uraian' => $this->uraian,
            'sumber_dana' => $this->sumber_dana,
            'koefisien' => $this->koefisien,
            'satuan' => $this->satuan,
            'harga' => $this->harga,
            'jumlah' => $this->jumlah,
            'jumlah_sebelum' => $this->jumlah_sebelum,
            'jumlah_setelah' => $this->jumlah_setelah,
            'selisih' => $this->selisih,
            'raw_line' => $this->raw_line,
            'sort_order' => $this->sort_order,
        ];
    }
}
