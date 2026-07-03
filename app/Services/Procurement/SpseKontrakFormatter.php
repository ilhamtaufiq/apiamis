<?php

namespace App\Services\Procurement;

use Carbon\CarbonInterface;

class SpseKontrakFormatter
{
    public function formatDate(?CarbonInterface $date): string
    {
        return $date ? $date->format('d-m-Y') : '';
    }

    public function formatNilai(?float $nilai): string
    {
        if ($nilai === null) {
            return '0,00';
        }

        return number_format($nilai, 2, ',', '');
    }

    public function formatJaminan(?float $nilai): string
    {
        return $this->formatNilai($nilai ?? 0);
    }

    public function parseNilai(?string $spseValue): ?float
    {
        if ($spseValue === null || trim($spseValue) === '') {
            return null;
        }

        $normalized = str_replace('.', '', trim($spseValue));
        $normalized = str_replace(',', '.', $normalized);

        if (! is_numeric($normalized)) {
            return null;
        }

        return (float) $normalized;
    }
}