<?php

namespace App\Services\Procurement;

/**
 * Default field SPSE — diselaraskan dengan skrip Python (sppbj.py, SPK_SPMK.py).
 */
class SpseFieldDefaults
{
    /**
     * Nilai default mengikuti sppbj.py dan SPK_SPMK.py.
     *
     * @var array<string, string>
     */
    private const VALUES = [
        'satker_kota' => 'Cianjur',
        'satker_alamat' => 'Jl. Adi Sucipta No. 7 - Cianjur',
        'ppk_nama' => 'AGUNG DELI SAHPUTRA, ST',
        'ppk_nip' => '197711212006041010',
        'ppk_jabatan' => 'Kepala Bidang',
        'ppk_no_sk' => '800.1.3.3/Kep.411/BKPSDM/10/2025',
        'cara_pembayaran' => 'Sekaligus',
        'waktu_penyelesaian' => '60 Hari Kalender',
        'sppbj_lamp' => '-',
        'jaminan_pelaksanaan' => '0,00',
        'masa_berlaku_jaminan' => '0',
        'lingkup_pekerjaan' => '<p>Sesuai Spesifikasi Teknis Pekerjaan</p>',
        'jabatan_wakil' => 'Direktur',
        'bank' => 'BJB',
        'norek' => '0',
    ];

    public static function defaultFor(string $key): string
    {
        return self::VALUES[$key] ?? '';
    }

    public static function get(string $key): string
    {
        if (function_exists('config')) {
            $configured = config('services.spse.'.$key);
            $trimmed = trim((string) ($configured ?? ''));

            if ($trimmed !== '') {
                return $trimmed;
            }
        }

        return self::defaultFor($key);
    }
}