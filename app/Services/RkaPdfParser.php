<?php

namespace App\Services;

use Illuminate\Support\Facades\File;
use Illuminate\Support\Str;
use RuntimeException;

class RkaPdfParser
{
    public function parse(string $pdfPath, string $jenis): array
    {
        $textPath = storage_path('app/rka/text/'.Str::uuid().'.txt');
        File::ensureDirectoryExists(dirname($textPath));

        $this->extractText($pdfPath, $textPath);

        $text = File::get($textPath);
        $lines = collect(preg_split('/\R/u', $text) ?: [])
            ->map(fn ($line) => rtrim($line))
            ->values()
            ->all();

        return [
            'text_path' => $textPath,
            'meta' => $this->parseMeta($lines, $jenis),
            'items' => $this->parseItems($lines, $jenis),
        ];
    }

    private function extractText(string $pdfPath, string $textPath): void
    {
        $command = 'pdftotext -layout '.escapeshellarg($pdfPath).' '.escapeshellarg($textPath);
        exec($command, $output, $exitCode);

        if ($exitCode !== 0 || ! File::exists($textPath)) {
            throw new RuntimeException('Gagal membaca PDF RKA. Pastikan pdftotext tersedia di server.');
        }
    }

    private function parseMeta(array $lines, string $jenis): array
    {
        $text = implode("\n", $lines);
        $sources = [];

        foreach ($lines as $index => $line) {
            if (preg_match('/Nomor\s+DPP?A\s*:\s*(.+)$/i', $line, $match)) {
                $nomorDokumen = trim($match[1]);
            }

            if (preg_match('/Tahun\s+Anggaran\s+(\d{4})/i', $line, $match) || preg_match('/TAHUN\s+ANGGARAN\s+(\d{4})/i', $line, $match)) {
                $tahun = $match[1];
            }

            if (preg_match('/^\s*Program\s*:\s*(.+)$/i', $line, $match)) {
                $program = trim($match[1]);
            }

            if (preg_match('/^\s*Kegiatan\s*:\s*(.+)$/i', $line, $match)) {
                $kegiatan = trim($match[1]);
            }

            if (preg_match('/^\s*(?:Sub\s+Kegiatan|Keluaran\s+Sub\s+Kegiatan)\s*:\s*(.+)$/i', $line, $match)) {
                $subKegiatan = trim($match[1]);
            }

            if (preg_match('/Sumber\s+(?:Pendanaan|Dana)\s*:?\s*(.+)$/i', $line, $match)) {
                $source = trim($match[1]);
                if ($source !== '') {
                    $sources[] = $source;
                }

                for ($offset = 1; $offset <= 3; $offset++) {
                    $next = trim($lines[$index + $offset] ?? '');
                    if (! str_starts_with($next, ':')) {
                        continue;
                    }

                    $next = trim(ltrim($next, ':'));
                    if ($next !== '') {
                        $sources[] = $next;
                    }
                }
            }
        }

        preg_match_all('/Rp\.?\s*[\d.]+,\d{2}/i', $text, $moneyMatches);
        $moneyValues = array_map(fn ($value) => $this->moneyToFloat($value), $moneyMatches[0] ?? []);

        return [
            'jenis' => $jenis,
            'nomor_dokumen' => $nomorDokumen ?? null,
            'tahun_anggaran' => $tahun ?? null,
            'program' => $this->cleanLabelValue($program ?? null),
            'kegiatan' => $this->cleanLabelValue($kegiatan ?? null),
            'sub_kegiatan' => $this->cleanLabelValue($subKegiatan ?? null),
            'sumber_pendanaan' => array_values(array_unique(array_filter(array_map([$this, 'cleanLabelValue'], $sources)))),
            'total_sebelum' => $jenis === 'parsial' ? ($moneyValues[0] ?? null) : null,
            'total_setelah' => $jenis === 'parsial' ? ($moneyValues[1] ?? null) : ($moneyValues[0] ?? null),
            'total_selisih' => $jenis === 'parsial' ? ($moneyValues[2] ?? null) : null,
        ];
    }

    private function parseItems(array $lines, string $jenis): array
    {
        $items = [];
        $currentSourceDana = null;
        $currentKodeRekening = null;
        $sort = 1;

        foreach ($lines as $index => $line) {
            $trimmed = trim(preg_replace('/\s+/u', ' ', $line) ?? '');

            if ($trimmed === '' || $this->isHeaderLine($trimmed)) {
                continue;
            }

            if (preg_match('/Sumber\s+Dana\s*:?\s*(.+)$/i', $trimmed, $match)) {
                $currentSourceDana = $this->cleanLabelValue($match[1]);

                continue;
            }

            if (preg_match('/^(\d(?:\.\d+)+)\s+(.+)$/', $trimmed, $match)) {
                $currentKodeRekening = $match[1];
                $values = $this->extractMoneyValues($trimmed);
                $items[] = [
                    'kode_rekening' => $currentKodeRekening,
                    'tipe' => 'rekening',
                    'uraian' => $this->stripMoney($match[2]),
                    'sumber_dana' => $currentSourceDana,
                    ...$this->amountColumns($values, $jenis),
                    'raw_line' => $line,
                    'sort_order' => $sort++,
                ];

                continue;
            }

            if (preg_match('/^\[\s*([#-])\s*\]\s*(.+)$/u', $trimmed, $match)) {
                $values = $this->extractMoneyValues($trimmed);
                $items[] = [
                    'kode_rekening' => $currentKodeRekening,
                    'tipe' => $match[1] === '#' ? 'kelompok' : 'paket',
                    'uraian' => $this->stripMoney($match[2]),
                    'sumber_dana' => $currentSourceDana,
                    ...$this->amountColumns($values, $jenis),
                    'raw_line' => $line,
                    'sort_order' => $sort++,
                ];

                continue;
            }

            $values = $this->extractMoneyValues($trimmed);
            if ($values !== [] && ! str_contains(strtolower($trimmed), 'spesifikasi')) {
                $items[] = [
                    'kode_rekening' => $currentKodeRekening,
                    'tipe' => 'rincian',
                    'uraian' => $this->stripMoney($trimmed),
                    'sumber_dana' => $currentSourceDana,
                    ...$this->amountColumns($values, $jenis),
                    'raw_line' => $line,
                    'sort_order' => $sort++,
                ];
            }
        }

        return $items;
    }

    private function isHeaderLine(string $line): bool
    {
        return str_contains($line, 'Rincian Perhitungan')
            || str_contains($line, 'Kode Rekening')
            || str_contains($line, 'Koefisien')
            || str_contains($line, 'Satuan Kerja Perangkat Daerah');
    }

    private function extractMoneyValues(string $line): array
    {
        preg_match_all('/\(?\s*Rp\.?\s*[\d.]+,\d{2}\s*\)?/i', $line, $matches);

        return array_map(fn ($value) => $this->moneyToFloat($value), $matches[0] ?? []);
    }

    private function amountColumns(array $values, string $jenis): array
    {
        if ($jenis === 'parsial') {
            $last = array_slice($values, -3);

            return [
                'jumlah_sebelum' => $last[0] ?? null,
                'jumlah_setelah' => $last[1] ?? null,
                'selisih' => $last[2] ?? null,
                'jumlah' => $last[1] ?? null,
            ];
        }

        $amount = $values ? end($values) : null;

        return [
            'jumlah_sebelum' => null,
            'jumlah_setelah' => null,
            'selisih' => null,
            'jumlah' => $amount,
        ];
    }

    private function moneyToFloat(string $value): float
    {
        $negative = str_contains($value, '(') && str_contains($value, ')');
        $clean = preg_replace('/[^\d,]/', '', $value) ?? '0';
        $number = (float) str_replace(',', '.', str_replace('.', '', $clean));

        return $negative ? -$number : $number;
    }

    private function stripMoney(string $value): string
    {
        $value = preg_replace('/\(?\s*Rp\.?\s*[\d.]+,\d{2}\s*\)?/i', '', $value) ?? $value;

        return $this->cleanLabelValue($value) ?: '-';
    }

    private function cleanLabelValue(?string $value): ?string
    {
        if ($value === null) {
            return null;
        }

        $value = trim(preg_replace('/\s+/u', ' ', $value) ?? $value);
        $value = preg_replace('/^\d+(?:\.\d+)*\s*[-:]?\s*/', '', $value) ?? $value;

        return trim($value) ?: null;
    }
}
