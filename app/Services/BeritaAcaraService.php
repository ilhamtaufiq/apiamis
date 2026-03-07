<?php

namespace App\Services;

use App\Models\DocumentSequence;
use Illuminate\Support\Facades\DB;

class BeritaAcaraService
{
    /**
     * Generate next document number based on type and year
     * 
     * @param string $docType
     * @param int|null $year
     * @return string
     */
    public function generateNextNumber(string $docType, ?int $year = null, ?int $pekerjaanId = null, ?int $kontrakId = null, bool $saveToDb = true): string
    {
        // If year is not provided, try to resolve it from pekerjaanId
        if (!$year && $pekerjaanId) {
            $pekerjaan = \App\Models\Pekerjaan::with('kegiatan')->find($pekerjaanId);
            if ($pekerjaan && $pekerjaan->kegiatan) {
                $year = (int) $pekerjaan->kegiatan->tahun_anggaran;
            }
        }

        // Default to current year if still null
        $year = $year ?? (int) date('Y');

        return DB::transaction(function () use ($docType, $year, $pekerjaanId, $kontrakId) {
            // Get or create sequence for the year and lock it
            $sequence = DocumentSequence::where('year', $year)
                ->lockForUpdate()
                ->first();

            if (!$sequence) {
                $sequence = DocumentSequence::create([
                    'year' => $year,
                    'last_number' => 0
                ]);
            }

            if ($saveToDb) {
                // Increment counter permanently
                $sequence->increment('last_number');
                $nextNumber = $sequence->last_number;
            } else {
                // Just peek at the next number
                $nextNumber = $sequence->last_number + 1;
            }

            $n = str_pad($nextNumber, 2, '0', STR_PAD_LEFT);
            $n3 = str_pad($nextNumber, 3, '0', STR_PAD_LEFT);

            // Handle Kontrak ID for templates
            if (!$kontrakId && $pekerjaanId) {
                // Try to find if there's already a contract for this work
                $kontrakId = \App\Models\Kontrak::where('id_pekerjaan', $pekerjaanId)->orderBy('id', 'desc')->value('id');
            }
            
            // If still no kontrakId (e.g. creating new), we use next predicted ID or just 0
            if (!$kontrakId) {
                $maxId = \App\Models\Kontrak::max('id') ?? 0;
                $kontrakId = $maxId + 1;
            }
            $k3 = str_pad($kontrakId, 3, '0', STR_PAD_LEFT);

            // Template mapping
            $templates = [
                'ba_lpp' => "600/BA.LPP.{$n}/{$year}",
                'stp_a'  => "600/{$n}/Disperkim",
                'stp_b'  => "600/PPHP.{$n}/Disperkim",
                'ba_php' => "600/BA. PHP/{$n}/{$year}",
                'ba_stp' => "600/BA.STP.{$n}/{$year}",
                'sppbj'  => "602.4/SPPBJ/PPK/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'spk'    => "602.4/SPK/PPK/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'spmk'   => "602.4/SPMK/PPK/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
            ];

            return $templates[$docType] ?? "UNKNOWN/{$n}";
        });
    }
}
