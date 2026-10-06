<?php

namespace App\Support;

/**
 * Helper satuan output pekerjaan.
 *
 * Volume output hanya boleh dibaca sebagai jumlah unit / KK bila satuannya
 * berupa hitungan (unit, bh, SR, KK, titik, ...). Satuan panjang/luas/volume
 * (m, m2, m3), lumpsum (LS), paket, dan berat tidak boleh — mis. "Pipa 300 m"
 * bukan 300 unit/KK.
 */
class OutputSatuan
{
    /** @var list<string> */
    private const NON_COUNT = [
        'm', 'm1', 'm2', 'm3', 'mtr', 'meter', 'meter persegi', 'meter kubik',
        'km', 'cm', 'mm', 'ha', 'are',
        'ls', 'lumpsum', 'lump sum', 'paket', 'pkt', 'set',
        'kg', 'ton', 'liter', 'ltr', 'l',
        'hari', 'bulan', 'minggu', 'jam', 'oh', 'hok',
    ];

    public static function isCountable(?string $satuan): bool
    {
        $normalized = mb_strtolower(trim((string) $satuan));
        if ($normalized === '') {
            // Satuan tidak diisi: pertahankan perilaku lama (anggap hitungan).
            return true;
        }

        $normalized = str_replace(['²', '³', "'", '’', '.'], ['2', '3', '', '', ''], $normalized);
        $normalized = preg_replace('/\s+/', ' ', $normalized) ?? $normalized;

        return ! in_array($normalized, self::NON_COUNT, true);
    }
}
