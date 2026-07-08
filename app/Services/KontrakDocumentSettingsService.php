<?php

namespace App\Services;

use App\Models\AppSetting;
use App\Services\Procurement\SpseFieldDefaults;

class KontrakDocumentSettingsService
{
    public const SETTING_NAMA_PPK = 'kontrak_nama_ppk';

    public const SETTING_NIP_PPK = 'kontrak_nip_ppk';

    public const SETTING_NAMA_PPTK = 'kontrak_nama_pptk';

    public const SETTING_NIP_PPTK = 'kontrak_nip_pptk';

    public const SETTING_MASA_PEMELIHARAAN_HARI = 'kontrak_masa_pemeliharaan_hari';

    public const SETTING_SKPD = 'kontrak_skpd';

    public const SETTING_NOMOR_DPA = 'kontrak_nomor_dpa';

    public const SETTING_TANGGAL_DPA = 'kontrak_tanggal_dpa';

    public const SETTING_CARA_PEMBAYARAN = 'kontrak_cara_pembayaran';

    public const CARA_PEMBAYARAN_SEKALIGUS = 'sekaligus';

    public const CARA_PEMBAYARAN_TERMIN = 'termin';

    public const CARA_PEMBAYARAN_BULAN = 'bulan';

    /**
     * @return array{nama_ppk: string, nip_ppk: string, nama_pptk: string, nip_pptk: string}
     */
    public function pejabatDefaults(): array
    {
        return [
            'nama_ppk' => $this->valueOrFallback(self::SETTING_NAMA_PPK, SpseFieldDefaults::get('ppk_nama')),
            'nip_ppk' => $this->valueOrFallback(self::SETTING_NIP_PPK, SpseFieldDefaults::get('ppk_nip')),
            'nama_pptk' => $this->valueOrFallback(self::SETTING_NAMA_PPTK, '-'),
            'nip_pptk' => $this->valueOrFallback(self::SETTING_NIP_PPTK, '-'),
        ];
    }

    /**
     * @return array{skpd: string, nomor_dpa: string, tanggal_dpa: string}
     */
    public function instansiDefaults(): array
    {
        return [
            'skpd' => $this->valueOrFallback(
                self::SETTING_SKPD,
                'Dinas Perumahan dan Kawasan Permukiman'
            ),
            'nomor_dpa' => $this->valueOrFallback(self::SETTING_NOMOR_DPA, '-'),
            'tanggal_dpa' => $this->valueOrFallback(self::SETTING_TANGGAL_DPA, '-'),
        ];
    }

    public function caraPembayaran(): string
    {
        $configured = strtolower(trim((string) (AppSetting::getValue(self::SETTING_CARA_PEMBAYARAN) ?? '')));

        if (in_array($configured, [
            self::CARA_PEMBAYARAN_SEKALIGUS,
            self::CARA_PEMBAYARAN_TERMIN,
            self::CARA_PEMBAYARAN_BULAN,
        ], true)) {
            return $configured;
        }

        $spseDefault = strtolower(trim(SpseFieldDefaults::get('cara_pembayaran')));

        return match ($spseDefault) {
            'termin' => self::CARA_PEMBAYARAN_TERMIN,
            'bulan' => self::CARA_PEMBAYARAN_BULAN,
            default => self::CARA_PEMBAYARAN_SEKALIGUS,
        };
    }

    /**
     * @return array<string, string>
     */
    public function caraPembayaranCheckboxData(): array
    {
        $selected = $this->caraPembayaran();
        $checked = '☑';
        $unchecked = '☐';

        return [
            'cara_pembayaran' => ucfirst($selected),
            'check_pembayaran_sekaligus' => $selected === self::CARA_PEMBAYARAN_SEKALIGUS ? $checked : $unchecked,
            'check_pembayaran_termin' => $selected === self::CARA_PEMBAYARAN_TERMIN ? $checked : $unchecked,
            'check_pembayaran_bulan' => $selected === self::CARA_PEMBAYARAN_BULAN ? $checked : $unchecked,
        ];
    }

    public function masaPemeliharaanHari(): int
    {
        $configured = AppSetting::getValue(self::SETTING_MASA_PEMELIHARAAN_HARI);
        $hari = (int) ($configured ?: 180);

        return max(1, $hari);
    }

    private function valueOrFallback(string $key, string $fallback): string
    {
        $value = trim((string) (AppSetting::getValue($key) ?? ''));

        return $value !== '' ? $value : $fallback;
    }
}