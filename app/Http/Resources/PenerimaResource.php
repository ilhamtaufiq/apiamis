<?php

namespace App\Http\Resources;

use Illuminate\Http\Resources\Json\JsonResource;

class PenerimaResource extends JsonResource
{
    public function toArray($request)
    {
        $pin = $request->header('X-PIN') ?? $request->query('pin');
        $unmasked = $pin === '123456';

        return [
            'id' => $this->id,
            'nama' => $this->nama,
            'jumlah_jiwa' => $this->jumlah_jiwa,
            'nik' => $unmasked ? $this->nik : $this->mask($this->nik, 4),
            'alamat' => $unmasked ? $this->alamat : $this->mask($this->alamat, 6),
            'is_komunal' => $this->is_komunal,
            'pekerjaan_id' => $this->pekerjaan_id,
            'pekerjaan' => new PekerjaanResource($this->whenLoaded('pekerjaan')),
            'created_at' => $this->created_at?->format('Y-m-d H:i:s'),
            'updated_at' => $this->updated_at?->format('Y-m-d H:i:s'),
        ];
    }

    private function mask(?string $val, int $show): ?string
    {
        if (!$val) {
            return $val;
        }
        $len = strlen($val);
        return $len <= $show
            ? str_repeat('*', $len)
            : substr($val, 0, $show) . str_repeat('*', min($len - $show, 12));
    }
}
