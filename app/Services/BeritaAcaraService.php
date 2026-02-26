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
    public function generateNextNumber(string $docType, ?int $year = null): string
    {
        $year = $year ?? (int) date('Y');

        return DB::transaction(function () use ($docType, $year) {
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

            // Increment counter
            $sequence->increment('last_number');
            $n = str_pad($sequence->last_number, 2, '0', STR_PAD_LEFT);

            // Template mapping
            $templates = [
                'ba_lpp' => "600/BA.LPP.{$n}/{$year}",
                'stp_a'  => "600/{$n}/Disperkim",
                'stp_b'  => "600/PPHP.{$n}/Disperkim",
                'ba_php' => "600/BA. PHP/{$n}/{$year}",
                'ba_stp' => "600/BA.STP.{$n}/{$year}",
            ];

            return $templates[$docType] ?? "UNKNOWN/{$n}";
        });
    }
}
