<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PekerjaanProgressEstimasiResource extends JsonResource
{
    public function toArray(Request $request): array
    {
        return [
            'pekerjaan_id' => $this->resource['pekerjaan_id'],
            'tahun_anggaran' => $this->resource['tahun_anggaran'],
            'fisik' => $this->resource['fisik'],
            'keuangan' => $this->resource['keuangan'],
            'updated_at' => $this->resource['updated_at'] ?? null,
        ];
    }
}