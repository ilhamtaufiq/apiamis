<?php

namespace Database\Seeders;

use App\Models\Desa;
use Illuminate\Database\Seeder;
use Illuminate\Support\Collection;

class DesaPopulationSeeder extends Seeder
{
    /**
     * Sync jumlah_penduduk in tbl_desa using kecamatan + desa names as keys.
     */
    public function run(): void
    {
        $rows = require database_path('seeders/data/desa_population.php');
        $lookup = $this->buildDesaLookup();
        $updated = 0;
        $unmatched = [];
        $ambiguous = [];

        foreach ($rows as $row) {
            $key = $this->normalize($row['kecamatan']).'|'.$this->normalize($row['desa']);
            $candidates = $lookup->get($key, collect());

            if ($candidates->isEmpty()) {
                $unmatched[] = $row;

                continue;
            }

            if ($candidates->count() > 1) {
                $ambiguous[] = $row;

                continue;
            }

            Desa::query()
                ->whereKey($candidates->first()->id)
                ->update(['jumlah_penduduk' => $row['jumlah_penduduk']]);

            $updated++;
        }

        $this->command?->info("Desa population seed synced {$updated} rows.");

        foreach (array_slice($unmatched, 0, 20) as $row) {
            $this->command?->warn("UNMATCHED {$row['kecamatan']} / {$row['desa']}");
        }

        foreach (array_slice($ambiguous, 0, 20) as $row) {
            $this->command?->warn("AMBIGUOUS {$row['kecamatan']} / {$row['desa']}");
        }
    }

    private function buildDesaLookup(): Collection
    {
        return Desa::with('kecamatan')
            ->get()
            ->groupBy(fn (Desa $desa): string => $this->normalize((string) $desa->kecamatan?->n_kec).'|'.$this->normalize((string) $desa->n_desa));
    }

    private function normalize(?string $value): string
    {
        if (! $value) {
            return '';
        }

        $value = str_ireplace(['kecamatan', 'desa', 'kelurahan'], '', $value);

        return strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
    }
}
