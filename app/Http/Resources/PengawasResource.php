<?php

namespace App\Http\Resources;

use Illuminate\Http\Request;
use Illuminate\Http\Resources\Json\JsonResource;

class PengawasResource extends JsonResource
{
    /**
     * Transform the resource into an array.
     *
     * @return array<string, mixed>
     */
    public function toArray(Request $request): array
    {
        // Only count pekerjaan where this person is the main pengawas
        $pekerjaan = $this->pekerjaanAsPengawas;
        
        // Count total pekerjaan (paket pekerjaan)
        $jumlahLokasi = $pekerjaan->count();
        
        // Sum pagu
        $totalPagu = $pekerjaan->sum('pagu');

        return [
            'id' => $this->id,
            'nama' => $this->nama,
            'nip' => $this->nip,
            'jabatan' => $this->jabatan,
            'telepon' => $this->telepon,
            'jumlah_lokasi' => $jumlahLokasi,
            'total_pagu' => $totalPagu,
            'created_at' => $this->created_at,
            'updated_at' => $this->updated_at,
        ];
    }
}
