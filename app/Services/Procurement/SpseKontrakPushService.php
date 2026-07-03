<?php

namespace App\Services\Procurement;

use App\Models\Kontrak;
use App\Models\SpseSession;

class SpseKontrakPushService
{
    public function __construct(
        private readonly SpseHttpClient $httpClient,
        private readonly SpseKontrakHtmlParser $htmlParser,
        private readonly SpseKontrakFormatter $formatter,
    ) {
    }

    /**
     * @return array<string, mixed>
     */
    public function push(Kontrak $kontrak, SpseSession $session): array
    {
        $kontrak->loadMissing(['penyedia', 'pekerjaans']);

        $this->assertPushable($kontrak);

        $plId = trim((string) $kontrak->kode_paket);
        $steps = [];
        $ids = [
            'pl_id' => $plId,
            'sppbj_id' => $kontrak->spse_sppbj_id,
            'spk_id' => $kontrak->spse_spk_id,
            'rekanan_id' => $kontrak->spse_rekanan_id,
        ];

        $listPath = '/sppbj-pl/listsppbjpl?plId='.$plId;
        $listHtml = $this->httpClient->fetchPage($session, $listPath, '/beranda/nontender');
        $parsed = $this->htmlParser->extractIdsFromListHtml($listHtml);
        $ids['sppbj_id'] = $ids['sppbj_id'] ?: $parsed['sppbj_id'];
        $ids['spk_id'] = $ids['spk_id'] ?: $parsed['spk_id'];

        $sppbjFormPath = '/sppbj-pl/sppbjppkpl?plId='.$plId;
        $sppbjFormHtml = $this->httpClient->fetchPage($session, $sppbjFormPath, $listPath);
        $rekananId = $this->htmlParser->resolveRekananId(
            $sppbjFormHtml,
            (string) ($kontrak->penyedia?->nama ?? ''),
            $kontrak->spse_rekanan_id,
        );

        if (! $rekananId) {
            throw new \InvalidArgumentException(
                'Penyedia "'.$kontrak->penyedia?->nama.'" tidak ditemukan di form SPPBJ SPSE. Pastikan paket sudah punya pemenang di SPSE.',
            );
        }
        $ids['rekanan_id'] = $rekananId;

        $token = $this->httpClient->resolveAuthenticityToken($session, $sppbjFormPath);

        $steps[] = $this->runStep('pengecekan_blacklist', function () use ($session, $rekananId, $plId, $kontrak, $sppbjFormPath) {
            $tgl = $this->formatter->formatDate($kontrak->tgl_sppbj ?? $kontrak->tgl_spk ?? now());
            $path = '/sppbj-pl/pengecekanblacklist?rknId='.$rekananId.'&tglbuat='.$tgl.'&llsId='.$plId;
            $this->httpClient->postAction($session, $path, $sppbjFormPath);

            return ['rekanan_id' => $rekananId];
        });

        $steps[] = $this->runStep('simpan_sppbj', function () use ($session, $token, $plId, $kontrak, $rekananId, $sppbjFormPath) {
            $fields = [
                'authenticityToken' => $token,
                'sppbj.sppbj_no' => (string) ($kontrak->sppbj ?? ''),
                'sppbj.sppbj_lamp' => '-',
                'sppbj.sppbj_tgl_kirim' => $this->formatter->formatDate($kontrak->tgl_sppbj ?? $kontrak->tgl_spk),
                'sppbj.sppbj_kota' => config('services.spse.satker_kota', 'Cianjur'),
                'sppbj.jabatan_ppk_sppbj' => config('services.spse.ppk_jabatan', 'Kepala Bidang'),
                'sppbj.alamat_satker' => config('services.spse.satker_alamat', ''),
                'rekananId' => $rekananId,
                'sppbj.jaminan_pelaksanaan' => $this->formatter->formatJaminan(0),
                'sppbj.masa_berlaku_jaminan' => '0',
            ];

            $result = $this->httpClient->postMultipart(
                $session,
                '/sppbj-pl/simpansppbjpl?plId='.$plId,
                $fields,
                $sppbjFormPath,
            );
            $this->assertSaveOk($result, 'SPPBJ');

            return ['status_code' => $result['status']];
        });

        $listHtml = $this->httpClient->fetchPage($session, $listPath, '/beranda/nontender');
        $parsed = $this->htmlParser->extractIdsFromListHtml($listHtml);
        $sppbjId = $parsed['sppbj_id'] ?? $ids['sppbj_id'];
        if (! $sppbjId) {
            throw new \RuntimeException('sppbjId tidak ditemukan setelah simpan SPPBJ.');
        }
        $ids['sppbj_id'] = $sppbjId;

        $spkFormPath = '/spk-pl/spkpl?sppbjId='.$sppbjId;
        $spkFormHtml = $this->httpClient->fetchPage($session, $spkFormPath, $listPath);
        $existingSpkId = $kontrak->spse_spk_id ?: $this->htmlParser->extractHiddenValue($spkFormHtml, 'spk.spk_id');
        $spkNilaiSpse = $this->htmlParser->extractNilaiKontrak($spkFormHtml);
        if ($spkNilaiSpse === null || trim($spkNilaiSpse) === '') {
            throw new \InvalidArgumentException(
                'Nilai kontrak tidak ditemukan di form SPK SPSE. Pastikan paket sudah memiliki nilai penawaran/pemenang di SPSE.',
            );
        }
        $nilaiKontrakSpse = $this->formatter->parseNilai($spkNilaiSpse);
        if ($nilaiKontrakSpse === null || $nilaiKontrakSpse == 0.0) {
            throw new \InvalidArgumentException(
                'Nilai kontrak di SPSE kosong atau 0. PDN/UMK tidak dapat diisi otomatis.',
            );
        }
        $token = $this->httpClient->resolveAuthenticityToken($session, $spkFormPath);

        $penyedia = $kontrak->penyedia;
        $steps[] = $this->runStep('simpan_spk', function () use (
            $session,
            $token,
            $sppbjId,
            $kontrak,
            $spkFormPath,
            $existingSpkId,
            $penyedia,
            $spkNilaiSpse,
        ) {
            $fields = [
                'authenticityToken' => $token,
                'spk.kontrak_lingkup_pekerjaan' => '<p>-</p>',
                'spk.spk_id' => (string) ($existingSpkId ?? ''),
                'spk.spk_no' => (string) ($kontrak->spk ?? ''),
                'spk.spk_tgl' => $this->formatter->formatDate($kontrak->tgl_spk),
                'content.kota_pesanan' => config('services.spse.satker_kota', 'Cianjur'),
                'spk.nama_ppk_kontrak' => config('services.spse.ppk_nama', ''),
                'spk.nip_ppk_kontrak' => config('services.spse.ppk_nip', ''),
                'spk.jabatan_ppk_kontrak' => config('services.spse.ppk_jabatan', ''),
                'spk.no_sk_ppk_kontrak' => config('services.spse.ppk_no_sk', ''),
                'spk.spk_wakil_penyedia' => (string) ($penyedia?->direktur ?: $penyedia?->nama ?? ''),
                'spk.spk_jabatan_wakil' => 'Direktur',
                'spk.spk_nama_bank' => (string) ($penyedia?->bank ?? ''),
                'spk.spk_norekening' => (string) ($penyedia?->norek ?? '0'),
                'spk.spk_nilai' => $spkNilaiSpse,
                'spk.nilai_pdn' => $spkNilaiSpse,
                'spk.nilai_umk' => $spkNilaiSpse,
                'ubahnilai' => 'false',
                'spk.alasanubah_s' => '',
            ];

            $result = $this->httpClient->postMultipart(
                $session,
                '/spk-pl/simpanspk?sppbjId='.$sppbjId,
                $fields,
                $spkFormPath,
            );
            $this->assertSaveOk($result, 'SPK');

            return [
                'status_code' => $result['status'],
                'sppbj_id' => $sppbjId,
                'spk_nilai' => $spkNilaiSpse,
                'nilai_pdn' => $spkNilaiSpse,
                'nilai_umk' => $spkNilaiSpse,
            ];
        });

        $listHtml = $this->httpClient->fetchPage($session, $listPath, '/beranda/nontender');
        $parsed = $this->htmlParser->extractIdsFromListHtml($listHtml);
        $spkId = $parsed['spk_id'] ?? $existingSpkId;
        if (! $spkId) {
            throw new \RuntimeException('spkId tidak ditemukan setelah simpan SPK.');
        }
        $ids['spk_id'] = $spkId;

        $token = $this->httpClient->resolveAuthenticityToken($session, $listPath);
        $steps[] = $this->runStep('simpan_cara_pembayaran', function () use ($session, $token, $sppbjId, $listPath) {
            $fields = [
                'authenticityToken' => $token,
                'cara_pembayaran' => config('services.spse.cara_pembayaran', 'Sekaligus'),
                'jumlah_termin' => '',
                'jumlah_bulan' => '',
                'simpan' => 'simpan',
            ];

            $result = $this->httpClient->postMultipart(
                $session,
                '/sskk-pl/simpancarapembayaran?id='.$sppbjId,
                $fields,
                $listPath,
            );
            $this->assertSaveOk($result, 'cara pembayaran');

            return ['status_code' => $result['status']];
        });

        $spmkFormPath = '/spk-pl/spmknon?sppbjId='.$sppbjId;
        $token = $this->httpClient->resolveAuthenticityToken($session, $spmkFormPath);
        $tglSpmk = $kontrak->tgl_spmk ?? $kontrak->tgl_spk;

        $steps[] = $this->runStep('simpan_spmk', function () use (
            $session,
            $token,
            $spkId,
            $sppbjId,
            $kontrak,
            $spmkFormPath,
            $penyedia,
            $tglSpmk,
        ) {
            $fields = [
                'authenticityToken' => $token,
                'pesanan.pes_no' => (string) ($kontrak->spmk ?? ''),
                'pesanan.pes_tgl' => $this->formatter->formatDate($tglSpmk),
                'tgl_diterima' => $this->formatter->formatDate($tglSpmk),
                'content.waktu_penyelesaian' => config('services.spse.waktu_penyelesaian', '60 Hari Kalender'),
                'tgl_selesai' => $this->formatter->formatDate($kontrak->tgl_selesai),
                'content.kota_pesanan' => config('services.spse.satker_kota', 'Cianjur'),
                'content.wakil_sah_rekanan' => (string) ($penyedia?->direktur ?: $penyedia?->nama ?? ''),
                'content.jabatan_wakil_rekanan' => 'Direktur',
                'simpan' => '',
            ];

            $result = $this->httpClient->postMultipart(
                $session,
                '/spk-pl/simpansuratpesanannon?spkId='.$spkId.'&sppbjId='.$sppbjId,
                $fields,
                $spmkFormPath,
            );
            $this->assertSaveOk($result, 'SPMK');

            return [
                'status_code' => $result['status'],
                'spk_id' => $spkId,
                'sppbj_id' => $sppbjId,
            ];
        });

        $log = [
            'pushed_at' => now()->toIso8601String(),
            'pl_id' => $plId,
            'spk_nilai_spse' => $spkNilaiSpse,
            'steps' => $steps,
            'ids' => $ids,
        ];

        $kontrak->update([
            'spse_sppbj_id' => $ids['sppbj_id'],
            'spse_spk_id' => $ids['spk_id'],
            'spse_rekanan_id' => $ids['rekanan_id'],
            'spse_pushed_at' => now(),
            'spse_push_log' => $log,
        ]);

        return [
            'message' => 'Push kontrak ke SPSE selesai.',
            'kontrak_id' => $kontrak->id,
            'spse_ids' => $ids,
            'nilai_kontrak_spse' => $spkNilaiSpse,
            'steps' => $steps,
        ];
    }

    private function assertPushable(Kontrak $kontrak): void
    {
        if (empty(trim((string) $kontrak->kode_paket))) {
            throw new \InvalidArgumentException('kode_paket wajib diisi sebelum push ke SPSE.');
        }

        if (! $kontrak->id_penyedia || ! $kontrak->penyedia) {
            throw new \InvalidArgumentException('Penyedia wajib dipilih pada kontrak.');
        }

        if (empty(trim((string) $kontrak->spk)) && empty(trim((string) $kontrak->sppbj))) {
            throw new \InvalidArgumentException('Minimal nomor SPK atau SPPBJ harus diisi.');
        }
    }

    /**
     * @param  callable(): array<string, mixed>  $callback
     * @return array<string, mixed>
     */
    private function runStep(string $name, callable $callback): array
    {
        try {
            $detail = $callback();

            return array_merge([
                'step' => $name,
                'status' => 'ok',
            ], $detail);
        } catch (\Throwable $e) {
            throw new \RuntimeException("Langkah {$name} gagal: ".$e->getMessage(), 0, $e);
        }
    }

    /**
     * @param  array{status: int, body: string, location: ?string}  $result
     */
    private function assertSaveOk(array $result, string $label): void
    {
        $status = $result['status'];
        if (! in_array($status, [200, 302, 303], true)) {
            throw new \RuntimeException("Simpan {$label} gagal: HTTP {$status}");
        }

        $body = strtolower($result['body'] ?? '');
        if (str_contains($body, 'error') && str_contains($body, 'exception')) {
            throw new \RuntimeException("Simpan {$label} ditolak SPSE.");
        }
    }
}