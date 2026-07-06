<?php

namespace App\Services;

use App\Models\DocumentRegister;
use Carbon\Carbon;

class KontrakDocumentDataBuilder
{
    public function build($pekerjaan, $kontrak, array $overrideData = []): array
    {
        Carbon::setLocale('id');
        setlocale(LC_ALL, 'id_ID', 'id_ID.UTF-8', 'Indonesian');

        $kegiatan = $pekerjaan->kegiatan;
        $penyedia = $kontrak->penyedia;
        $kontrak->loadMissing(['registers.type']);
        $bastpRegister = $this->findRegisterByCode($kontrak, 'BASTP');

        $data = [
            'nama_paket' => $pekerjaan->nama_paket,
            'pagu' => 'Rp. '.number_format($pekerjaan->pagu, 0, ',', '.'),
            'pagu_terbilang' => $this->terbilang($pekerjaan->pagu),
            'kode_rekening' => $pekerjaan->kode_rekening,
            'kecamatan' => $pekerjaan->kecamatan ? $pekerjaan->kecamatan->nama : '-',
            'desa' => $pekerjaan->desa ? $pekerjaan->desa->nama : '-',
            'nama_program' => $kegiatan ? $kegiatan->nama_program : '-',
            'nama_kegiatan' => $kegiatan ? $kegiatan->nama_kegiatan : '-',
            'sub_kegiatan' => $kegiatan ? $kegiatan->nama_sub_kegiatan : '-',
            'nama_subkegiatan' => $kegiatan ? $kegiatan->nama_sub_kegiatan : '-',
            'nama_sub_kegiatan' => $kegiatan ? $kegiatan->nama_sub_kegiatan : '-',
            'tahun' => $kegiatan ? $kegiatan->tahun_anggaran : '-',
            'nilai_kontrak' => 'Rp. '.number_format($kontrak->nilai_kontrak, 0, ',', '.'),
            'nilai_kontrak_terbilang' => $this->terbilang($kontrak->nilai_kontrak),
            'terbilang_nilai_kontrak' => $this->terbilang($kontrak->nilai_kontrak),
            'tgl_sppbj' => $kontrak->tgl_sppbj instanceof Carbon ? $kontrak->tgl_sppbj->translatedFormat('d F Y') : '-',
            'tgl_spk' => $kontrak->tgl_spk instanceof Carbon ? $kontrak->tgl_spk->translatedFormat('d F Y') : '-',
            'tgl_selesai' => $kontrak->tgl_selesai instanceof Carbon ? $kontrak->tgl_selesai->translatedFormat('d F Y') : '-',
            'tgl_spmk' => $kontrak->tgl_spmk instanceof Carbon ? $kontrak->tgl_spmk->translatedFormat('d F Y') : '-',
            'tanggal_spk' => $kontrak->tgl_spk instanceof Carbon ? $kontrak->tgl_spk->translatedFormat('d F Y') : '-',
            'tanggal_mulai' => $kontrak->tgl_spmk instanceof Carbon ? $kontrak->tgl_spmk->translatedFormat('d F Y') : '-',
            'tanggal_selesai' => $kontrak->tgl_selesai instanceof Carbon ? $kontrak->tgl_selesai->translatedFormat('d F Y') : '-',
            'nomor_sppbj' => $kontrak->sppbj ?: '-',
            'nomor_spk' => $kontrak->spk ?: '-',
            'nomor_spmk' => $kontrak->spmk ?: '-',
            'kode_rup' => $kontrak->kode_rup ?: '-',
            'kode_paket' => $kontrak->kode_paket ?: '-',
            'nomor_penawaran' => $kontrak->nomor_penawaran ?: '-',
            'tanggal_penawaran' => $kontrak->tanggal_penawaran instanceof Carbon ? $kontrak->tanggal_penawaran->translatedFormat('d F Y') : '-',
            'nama_penyedia' => $penyedia ? $penyedia->nama : '-',
            'direktur' => $penyedia ? $penyedia->direktur : '-',
            'nama_direktur' => $penyedia ? $penyedia->direktur : '-',
            'alamat_penyedia' => $penyedia ? $penyedia->alamat : '-',
            'bank' => $penyedia ? $penyedia->bank : '-',
            'bank_penyedia' => $penyedia ? $penyedia->bank : '-',
            'norek' => $penyedia ? $penyedia->norek : '-',
            'rekening_penyedia' => $penyedia ? $penyedia->norek : '-',
            'npwp_penyedia' => $penyedia?->npwp ?? '-',
            'no_akta' => $penyedia ? $penyedia->no_akta : '-',
            'notaris' => $penyedia ? $penyedia->notaris : '-',
            'tanggal_akta' => $penyedia && $penyedia->tanggal_akta instanceof Carbon ? $penyedia->tanggal_akta->translatedFormat('d F Y') : '-',
            'nomor_bastp' => $bastpRegister?->nomor ?: '-',
            'tgl_bastp' => $bastpRegister && $bastpRegister->tanggal instanceof Carbon
                ? $bastpRegister->tanggal->translatedFormat('d F Y')
                : '-',
            'masa_hari' => ($kontrak->tgl_spmk instanceof Carbon && $kontrak->tgl_selesai instanceof Carbon)
                ? (int) $kontrak->tgl_spmk->diffInDays($kontrak->tgl_selesai)
                : '-',
            'masa' => ($kontrak->tgl_spmk instanceof Carbon && $kontrak->tgl_selesai instanceof Carbon)
                ? ((int) $kontrak->tgl_spmk->diffInDays($kontrak->tgl_selesai)).' Hari'
                : '-',
            'masa_hari_terbilang' => ($kontrak->tgl_spmk instanceof Carbon && $kontrak->tgl_selesai instanceof Carbon)
                ? $this->terbilang((int) $kontrak->tgl_spmk->diffInDays($kontrak->tgl_selesai))
                : '-',
        ];

        $data['Pekerjaan'] = $data['nama_paket'];
        $data['Penyedia'] = $data['nama_penyedia'];
        $data['Nama_SubKegiatan'] = $data['nama_subkegiatan'];
        $data['Nama_Sub_Kegiatan'] = $data['nama_sub_kegiatan'];
        $data['Nilai_Kontrak'] = $data['nilai_kontrak'];
        $data['Terbilang'] = $data['nilai_kontrak_terbilang'];
        $data['Kota'] = $pekerjaan->kecamatan ? $pekerjaan->kecamatan->nama : 'Cianjur';
        $data['SPK'] = $data['tgl_spk'];
        $data['SPK1'] = $data['nomor_spk'];
        $data['SPPBJ'] = $data['tgl_sppbj'];
        $data['SPPBJ1'] = $data['nomor_sppbj'];
        $data['Masa'] = $data['masa_hari'];
        $data['Selesai'] = $data['tgl_selesai'];
        $data['tgl_spl'] = $data['tgl_spk'];

        $data = array_merge($data, $this->sumberDanaCheckboxData($kegiatan?->sumber_dana));
        $data = array_merge($data, $this->addendumData($kontrak));

        if (! empty($overrideData)) {
            foreach ($overrideData as $key => $value) {
                $lowerKey = strtolower($key);
                $moneyKeywords = ['nilai', 'jumlah', 'dpp', 'ppn', 'total', 'tagihan'];
                $isMoney = false;
                foreach ($moneyKeywords as $mk) {
                    if (str_contains($lowerKey, $mk)) {
                        $isMoney = true;
                        break;
                    }
                }

                $excludeKeywords = ['nomor', 'tgl', 'tanggal', 'tahun', 'kode', 'id', 'rate', 'hari'];
                $isExcluded = false;
                if (! $isMoney) {
                    foreach ($excludeKeywords as $kw) {
                        if (str_contains($lowerKey, $kw)) {
                            $isExcluded = true;
                            break;
                        }
                    }
                }

                if (is_numeric($value)) {
                    if ($isMoney || ! $isExcluded) {
                        $data[$key] = 'Rp. '.number_format((float) $value, 0, ',', '.');
                    } else {
                        $data[$key] = $value;
                    }
                } else {
                    $data[$key] = $value;
                }
            }
        }

        return $data;
    }

    private function findRegisterByCode($kontrak, string $code): ?DocumentRegister
    {
        $normalized = strtoupper($code);

        return $kontrak->registers->first(function (DocumentRegister $register) use ($normalized) {
            return strtoupper((string) ($register->type?->code ?? '')) === $normalized;
        });
    }

    private function addendumData($kontrak): array
    {
        $addendums = $kontrak->relationLoaded('approvedAddendums')
            ? $kontrak->approvedAddendums
            : $kontrak->approvedAddendums()->get();

        $data = [];
        $maxSlots = max(10, $addendums->count());

        for ($slot = 1; $slot <= $maxSlots; $slot++) {
            $data["nomor_addendum{$slot}"] = '-';
            $data["nomor _addendum{$slot}"] = '-';
            $data["tgl_addendum{$slot}"] = '-';
            $data["tgl _addendum{$slot}"] = '-';
            $data["tanggal_addendum{$slot}"] = '-';
            $data["tanggal _addendum{$slot}"] = '-';
        }

        foreach ($addendums->values() as $index => $addendum) {
            $slot = $index + 1;
            $tanggal = $addendum->tanggal_addendum instanceof Carbon
                ? $addendum->tanggal_addendum->translatedFormat('d F Y')
                : ($addendum->tanggal_addendum ?: '-');
            $nomor = $addendum->nomor_addendum ?: '-';

            $data["nomor_addendum{$slot}"] = $nomor;
            $data["nomor _addendum{$slot}"] = $nomor;
            $data["tgl_addendum{$slot}"] = $tanggal;
            $data["tgl _addendum{$slot}"] = $tanggal;
            $data["tanggal_addendum{$slot}"] = $tanggal;
            $data["tanggal _addendum{$slot}"] = $tanggal;
        }

        return $data;
    }

    private function sumberDanaCheckboxData(?string $sumberDana): array
    {
        $normalized = strtoupper((string) $sumberDana);
        $normalized = preg_replace('/[^A-Z0-9]+/', ' ', $normalized);

        $checked = '☑';
        $unchecked = '☐';

        $hasApbd = str_contains($normalized, 'APBD');
        $hasApbn = str_contains($normalized, 'APBN');
        $hasDak = str_contains($normalized, 'DAK');
        $hasDau = str_contains($normalized, 'DAU');
        $hasDid = str_contains($normalized, 'DID');
        $hasBanprov = str_contains($normalized, 'BANPROV') || str_contains($normalized, 'BANTUAN PROVINSI');
        $hasDbh = str_contains($normalized, 'DBH') && ! str_contains($normalized, 'DBHCT') && ! str_contains($normalized, 'PAJAK ROKOK') && ! str_contains($normalized, 'PROV');
        $hasSilpa = str_contains($normalized, 'SILPA');
        $hasDbhPajakRokok = str_contains($normalized, 'DBH PAJAK ROKOK') || str_contains($normalized, 'PAJAK ROKOK');
        $hasPad = str_contains($normalized, 'PAD');
        $hasDbhct = str_contains($normalized, 'DBHCT');
        $hasDbhProv = str_contains($normalized, 'DBH PROV') || str_contains($normalized, 'DBH PROVINSI');

        return [
            'sumber_dana' => $sumberDana ?: '-',
            'checkbox_apbd' => $hasApbd ? $checked : $unchecked,
            'checkbox_ apbd' => $hasApbd ? $checked : $unchecked,
            'checkbox_apbn' => $hasApbn ? $checked : $unchecked,
            'checkbox_ apbn' => $hasApbn ? $checked : $unchecked,
            'checkbox_dak' => $hasDak ? $checked : $unchecked,
            'checkbox_dau' => $hasDau ? $checked : $unchecked,
            'checkbox_did' => $hasDid ? $checked : $unchecked,
            'checkbox_banprov' => $hasBanprov ? $checked : $unchecked,
            'checkbox_bantuan_provinsi' => $hasBanprov ? $checked : $unchecked,
            'checkbox_dbh' => $hasDbh ? $checked : $unchecked,
            'checkbox_silpa' => $hasSilpa ? $checked : $unchecked,
            'checkbox_dbh_pajak_rokok' => $hasDbhPajakRokok ? $checked : $unchecked,
            'checkbox_pad' => $hasPad ? $checked : $unchecked,
            'checkbox_dbhct' => $hasDbhct ? $checked : $unchecked,
            'checkbox_dbh_prov' => $hasDbhProv ? $checked : $unchecked,
            'check_apbd' => $hasApbd ? $checked : $unchecked,
            'check_ apbd' => $hasApbd ? $checked : $unchecked,
            'check_apbn' => $hasApbn ? $checked : $unchecked,
            'check_ apbn' => $hasApbn ? $checked : $unchecked,
            'check_dak' => $hasDak ? $checked : $unchecked,
            'check_dau' => $hasDau ? $checked : $unchecked,
            'check_did' => $hasDid ? $checked : $unchecked,
            'check_banprov' => $hasBanprov ? $checked : $unchecked,
            'check_ banprov' => $hasBanprov ? $checked : $unchecked,
            'check_bantuan_provinsi' => $hasBanprov ? $checked : $unchecked,
            'check_dbh' => $hasDbh ? $checked : $unchecked,
            'check_silpa' => $hasSilpa ? $checked : $unchecked,
            'check_dbh_pajak_rokok' => $hasDbhPajakRokok ? $checked : $unchecked,
            'check_pad' => $hasPad ? $checked : $unchecked,
            'check_dbhct' => $hasDbhct ? $checked : $unchecked,
            'check_dbh_prov' => $hasDbhProv ? $checked : $unchecked,
            'APBD_CHECK' => $hasApbd ? $checked : $unchecked,
            'APBN_CHECK' => $hasApbn ? $checked : $unchecked,
            'DAK_CHECK' => $hasDak ? $checked : $unchecked,
            'DAU_CHECK' => $hasDau ? $checked : $unchecked,
            'DID_CHECK' => $hasDid ? $checked : $unchecked,
            'BANPROV_CHECK' => $hasBanprov ? $checked : $unchecked,
            'BANTUAN_PROVINSI_CHECK' => $hasBanprov ? $checked : $unchecked,
            'DBH_CHECK' => $hasDbh ? $checked : $unchecked,
            'SILPA_CHECK' => $hasSilpa ? $checked : $unchecked,
            'DBH_PAJAK_ROKOK_CHECK' => $hasDbhPajakRokok ? $checked : $unchecked,
            'PAD_CHECK' => $hasPad ? $checked : $unchecked,
            'DBHCT_CHECK' => $hasDbhct ? $checked : $unchecked,
            'DBH_PROV_CHECK' => $hasDbhProv ? $checked : $unchecked,
        ];
    }

    private function terbilang($angka): string
    {
        $angka = (int) abs($angka);
        $baca = ['', 'Satu', 'Dua', 'Tiga', 'Empat', 'Lima', 'Enam', 'Tujuh', 'Delapan', 'Sembilan', 'Sepuluh', 'Sebelas'];
        $terbilang = '';

        if ($angka < 12) {
            $terbilang = $baca[$angka];
        } elseif ($angka < 20) {
            $terbilang = $this->terbilang($angka - 10).' Belas';
        } elseif ($angka < 100) {
            $terbilang = $this->terbilang((int) ($angka / 10)).' Puluh '.$this->terbilang($angka % 10);
        } elseif ($angka < 200) {
            $terbilang = 'Seratus '.$this->terbilang($angka - 100);
        } elseif ($angka < 1000) {
            $terbilang = $this->terbilang((int) ($angka / 100)).' Ratus '.$this->terbilang($angka % 100);
        } elseif ($angka < 2000) {
            $terbilang = 'Seribu '.$this->terbilang($angka - 1000);
        } elseif ($angka < 1000000) {
            $terbilang = $this->terbilang((int) ($angka / 1000)).' Ribu '.$this->terbilang($angka % 1000);
        } elseif ($angka < 1000000000) {
            $terbilang = $this->terbilang((int) ($angka / 1000000)).' Juta '.$this->terbilang($angka % 1000000);
        } elseif ($angka < 1000000000000) {
            $terbilang = $this->terbilang((int) ($angka / 1000000000)).' Milyar '.$this->terbilang($angka % 1000000000);
        } elseif ($angka < 1000000000000000) {
            $terbilang = $this->terbilang((int) ($angka / 1000000000000)).' Trilyun '.$this->terbilang($angka % 1000000000000);
        }

        return trim(preg_replace('/\s+/', ' ', $terbilang));
    }
}