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

        return DB::transaction(function () use ($docType, $year, $pekerjaanId, $kontrakId, $saveToDb) {
            // ALL documents now share a single global sequence as requested by user
            $sequenceType = 'kontrak_global';

            // Get or create sequence for the year and type, then lock it
            $sequence = DocumentSequence::where('year', $year)
                ->where('type', $sequenceType)
                ->lockForUpdate()
                ->first();

            if (!$sequence) {
                $sequence = DocumentSequence::create([
                    'year' => $year,
                    'type' => $sequenceType,
                    'last_number' => 0
                ]);
            }

            if ($saveToDb) {
                $sequence->increment('last_number');
                $nextNumber = $sequence->last_number;
            } else {
                $nextNumber = $sequence->last_number + 1;
            }

            $n = str_pad($nextNumber, 2, '0', STR_PAD_LEFT);
            $n3 = str_pad($nextNumber, 3, '0', STR_PAD_LEFT);

            if (!$kontrakId && $pekerjaanId) {
                $kontrakId = \App\Models\Kontrak::where('id_pekerjaan', $pekerjaanId)->orderBy('id', 'desc')->value('id');
            }
            
            if (!$kontrakId) {
                $maxId = \App\Models\Kontrak::max('id') ?? 0;
                $kontrakId = $maxId + 1;
            }
            $k3 = str_pad($kontrakId, 3, '0', STR_PAD_LEFT);

            // Template mapping with all types requested by user
            $templates = [
                'ba_lpp'  => "600/BA.LPP/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'stp_a'   => "600/STP-PHO/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'ba_php'  => "600/BA.PHP/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'ba_stp'  => "600/BA.STP/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'ba_final' => "600/BA.FINAL/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'sppbj'   => "602.4/SPPBJ/PPK/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'spk'     => "602.4/SPK/PPK/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'spk_add'  => "602.4/SPK/PPK-ADD/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
                'spmk'    => "602.4/SPMK/PPK/DISPERKIM-AMS.{$k3}-{$n3}/{$year}",
            ];

            $fullNumber = $templates[$docType] ?? "SURAT/{$n3}/{$year}";

            // Register in logs if saved
            if ($saveToDb) {
                DB::table('tbl_document_logs')->insert([
                    'type' => $docType,
                    'year' => $year,
                    'sequence_number' => $nextNumber,
                    'full_number' => $fullNumber,
                    'id_pekerjaan' => $pekerjaanId,
                    'id_user' => auth()->id() ?? 1,
                    'status' => 'active',
                    'created_at' => now(),
                    'updated_at' => now(),
                ]);
            }

            return $fullNumber;
        });
    }
}
