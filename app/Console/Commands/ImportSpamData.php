<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Illuminate\Support\Facades\DB;
use App\Models\Kecamatan;
use App\Models\Desa;
use App\Models\UnitSpam;
use App\Models\SpamBudget;

class ImportSpamData extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'spam:import-data {--file= : Path to the CSV file (defaults to local windows path)}';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Import ALL SPAM data (Desa, Unit SPAM, Pengelola, Budgets) from SPSE CSV file';

    /**
     * Execute the console command.
     */
    public function handle(): int
    {
        $csvPath = $this->option('file') ?: 'C:\Users\asusg\Downloads\spse_cianjur_keduanya_2013-2025_spam_wilayah.csv';

        if (!file_exists($csvPath)) {
            $this->error("CSV file not found at: {$csvPath}");
            return 1;
        }

        $this->info("Opening and parsing SPSE CSV file from: {$csvPath}...");

        $file = fopen($csvPath, 'r');
        if (!$file) {
            $this->error("Failed to open CSV file.");
            return 1;
        }

        // Read header
        $header = fgetcsv($file, 1000, ';');
        if (!$header) {
            $this->error("CSV file is empty.");
            fclose($file);
            return 1;
        }

        $this->info("Beginning transaction to import budget packages...");
        DB::beginTransaction();

        try {
            // Truncate existing budgets to ensure fresh consolidation
            SpamBudget::query()->delete();

            $rowCount = 0;
            $matchedCount = 0;
            $createdUnitCount = 0;
            $createdDesaCount = 0;

            while (($row = fgetcsv($file, 1000, ';')) !== false) {
                // Semicolon values: Nilai Kontrak;QueryYear;Nama Paket;Desa/Kelurahan;Kecamatan
                if (count($row) < 5) {
                    continue;
                }

                $nilaiStr = trim($row[0]);
                $tahun = trim($row[1]);
                $namaPaket = trim($row[2]);
                $desaName = trim($row[3]);
                $kecName = trim($row[4]);

                if (empty($desaName) || empty($kecName)) {
                    continue;
                }

                if (strtolower($kecName) === 'kaupandak') {
                    $kecName = 'Kadupandak';
                }

                $rowCount++;

                // 1. Match Kecamatan
                $kec = Kecamatan::whereRaw('LOWER(n_kec) = ?', [strtolower($kecName)])
                    ->first();

                // Typos fallback or fuzzy match (e.g. Campakamulya vs Campaka)
                if (!$kec) {
                    $kec = Kecamatan::where('n_kec', 'like', "%{$kecName}%")
                        ->first();
                }

                if (!$kec) {
                    $this->warn("Row {$rowCount}: Kecamatan '{$kecName}' not found. Skipping.");
                    continue;
                }

                // 2. Match Desa
                $desa = Desa::where('kecamatan_id', $kec->id)
                    ->where(function($q) use ($desaName) {
                        $q->whereRaw('LOWER(n_desa) = ?', [strtolower($desaName)]);
                    })
                    ->first();

                // Typos fallback (e.g. Cikidangbayabang vs Cikidangbayangbang)
                if (!$desa) {
                    $desa = Desa::where('kecamatan_id', $kec->id)
                        ->where(function($q) use ($desaName) {
                            $q->where('n_desa', 'like', "%{$desaName}%");
                        })
                        ->first();
                }

                // If still not found, dynamically create the Desa to hold the budget record!
                if (!$desa) {
                    $desa = Desa::create([
                        'kecamatan_id' => $kec->id,
                        'n_desa' => $desaName,
                        'bjp_master' => 0
                    ]);
                    $createdDesaCount++;
                }

                // 3. Match or create SPAM Unit for this village
                $unit = UnitSpam::where('desa_id', $desa->id)->first();

                if (!$unit) {
                    $unit = UnitSpam::create([
                        'desa_id' => $desa->id,
                        'name' => "SPAM " . strtoupper($desa->n_desa) . " " . strtoupper($kec->n_kec),
                        'is_simspam' => false,
                        'sistem_layanan' => 'Perpipaan',
                        'sumber_mata_air_kap' => '0',
                        'sumber_air_tanah_kap' => '0'
                    ]);

                    // Default pengelola
                    $unit->pengelola()->create([
                        'pokmas' => "KPSPAM " . strtoupper($desa->n_desa) . " " . strtoupper($kec->n_kec),
                        'kepala' => '-',
                        'bendahara' => '-',
                        'sekretaris' => '-',
                    ]);

                    $createdUnitCount++;
                }

                // 4. Parse contract value
                $cleanNilai = str_replace(['Rp', '.'], '', $nilaiStr);
                $parts = explode(',', $cleanNilai);
                $nilaiKontrak = (double)($parts[0] . (isset($parts[1]) ? '.' . $parts[1] : ''));

                // 5. Create budget record
                SpamBudget::create([
                    'unit_spam_id' => $unit->id,
                    'nilai_kontrak' => $nilaiKontrak,
                    'tahun' => $tahun,
                    'nama_paket' => $namaPaket,
                    'sumber_dana' => 'APBD', // Default is APBD as requested
                ]);

                $matchedCount++;
            }

            fclose($file);
            DB::commit();

            $this->info("SPAM Budgets imported successfully!");
            $this->info("- Total rows processed: {$rowCount}");
            $this->info("- Successfully mapped budgets: {$matchedCount}");
            $this->info("- Dynamically created villages (Desa): {$createdDesaCount}");
            $this->info("- Dynamically created SPAM units: {$createdUnitCount}");

            return 0;

        } catch (\Exception $e) {
            DB::rollBack();
            fclose($file);
            $this->error("Failed to import budgets: " . $e->getMessage());
            return 1;
        }
    }
}
