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
        // Combine pekerjaan where this person is pengawas or pendamping
        $allPekerjaan = $this->pekerjaanAsPengawas->merge($this->pekerjaanAsPendamping);
        
        // Count unique kecamatan
        $jumlahLokasi = $allPekerjaan->pluck('kecamatan_id')->unique()->filter()->count();
        
        // Sum pagu
        $totalPagu = $allPekerjaan->sum('pagu');

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
