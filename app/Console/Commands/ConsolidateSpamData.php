<?php

namespace App\Console\Commands;

use Illuminate\Console\Command;
use Illuminate\Support\Facades\DB;
use App\Models\Kecamatan;
use App\Models\Desa;
use App\Models\UnitSpam;
use App\Models\Pengelola;
use App\Models\UnitChecklist;
use App\Models\SpamAchievement;

class ConsolidateSpamData extends Command
{
    /**
     * The name and signature of the console command.
     *
     * @var string
     */
    protected $signature = 'spam:consolidate';

    /**
     * The console command description.
     *
     * @var string
     */
    protected $description = 'Consolidate SPAM and SPM data from SQLite spm_am.db database';

    /**
     * Execute the console command.
     */
    public function handle(): int
    {
        $dbPath = 'C:\\laragon\\www\\bun\\spm_am.db';
        
        if (!file_exists($dbPath)) {
            $this->error("SQLite database file not found at: {$dbPath}");
            return 1;
        }

        $this->info("Opening SQLite connection to {$dbPath}...");
        
        try {
            $sqlite = new \PDO("sqlite:" . $dbPath);
            $sqlite->setAttribute(\PDO::ATTR_ERRMODE, \PDO::ERRMODE_EXCEPTION);
        } catch (\Exception $e) {
            $this->error("Failed to connect to SQLite: " . $e->getMessage());
            return 1;
        }

        $this->info("Beginning consolidation transaction...");

        DB::beginTransaction();

        try {
            // Load all current MySQL districts & villages for matching
            $kecamatans = Kecamatan::all();
            $desas = Desa::all();

            $this->info("Loaded " . $kecamatans->count() . " districts and " . $desas->count() . " villages from MySQL.");

            // 1.5. Parse semua_desa.md BJP data
            $bjpPath = 'C:\\laragon\\www\\tools\\spm-am\\semua_desa.md';
            $bjpData = [];
            if (file_exists($bjpPath)) {
                $this->info("Parsing semua_desa.md for BJP master data...");
                $lines = file($bjpPath);
                foreach ($lines as $line) {
                    $parts = preg_split('/\s+/', trim($line));
                    if (count($parts) >= 3) {
                        $num = intval(end($parts));
                        $kecName = trim($parts[0]);
                        $desaName = trim($parts[1]);
                        
                        $key = $this->normalize($kecName) . '|' . $this->normalize($desaName);
                        $bjpData[$key] = $num;
                    }
                }
                $this->info("Parsed " . count($bjpData) . " BJP entries.");
            } else {
                $this->warn("semua_desa.md not found at {$bjpPath}. skipping BJP master load.");
            }

            // 1. Consolidate Desa Target
            $this->info("Consolidating Desa target figures...");
            $stmt = $sqlite->query("
                SELECT d.name as desa_name, k.name as kec_name, d.target
                FROM desa d
                JOIN kecamatan k ON d.kecamatan_id = k.id
            ");
            $sqliteDesas = $stmt->fetchAll(\PDO::FETCH_ASSOC);

            $matchedDesaCount = 0;
            $sqliteToMySqlDesaMap = []; // Map sqlite_desa_id => mysql_desa_id

            foreach ($sqliteDesas as $sDesa) {
                // Find matching Kecamatan
                $matchedKec = $kecamatans->first(function ($kec) use ($sDesa) {
                    return $this->normalize($kec->n_kec) === $this->normalize($sDesa['kec_name']);
                });

                if (!$matchedKec) {
                    continue;
                }

                // Find matching Desa
                $matchedDesa = $desas->first(function ($desa) use ($sDesa, $matchedKec) {
                    return $desa->kecamatan_id === $matchedKec->id && 
                           $this->normalize($desa->n_desa) === $this->normalize($sDesa['desa_name']);
                });

                if ($matchedDesa) {
                    $key = $this->normalize($matchedKec->n_kec) . '|' . $this->normalize($matchedDesa->n_desa);
                    $bjpMasterVal = $bjpData[$key] ?? 0;

                    $matchedDesa->update([
                        'target' => $sDesa['target'],
                        'bjp_master' => $bjpMasterVal
                    ]);
                    $matchedDesaCount++;
                }
            }

            $this->info("Updated target & bjp_master for {$matchedDesaCount} villages successfully.");

            // Create a mapping dictionary for SQLite Desa ID to MySQL Desa ID
            $sqliteDesaListStmt = $sqlite->query("SELECT id, name, kecamatan_id FROM desa");
            $sqliteDesaList = $sqliteDesaListStmt->fetchAll(\PDO::FETCH_ASSOC);
            
            // Query sqlite kecamatan to resolve names
            $sqliteKecListStmt = $sqlite->query("SELECT id, name FROM kecamatan");
            $sqliteKecList = $sqliteKecListStmt->fetchAll(\PDO::FETCH_KEY_PAIR);

            foreach ($sqliteDesaList as $sDesa) {
                $kecName = $sqliteKecList[$sDesa['kecamatan_id']] ?? '';
                $matchedKec = $kecamatans->first(function ($kec) use ($kecName) {
                    return $this->normalize($kec->n_kec) === $this->normalize($kecName);
                });

                if (!$matchedKec) continue;

                $matchedDesa = $desas->first(function ($desa) use ($sDesa, $matchedKec) {
                    return $desa->kecamatan_id === $matchedKec->id && 
                           $this->normalize($desa->n_desa) === $this->normalize($sDesa['name']);
                });

                if ($matchedDesa) {
                    $sqliteToMySqlDesaMap[$sDesa['id']] = $matchedDesa->id;
                }
            }

            // 2. Clear existing normalized tables
            $this->info("Clearing existing normalized tables...");
            SpamAchievement::query()->delete();
            UnitChecklist::query()->delete();
            Pengelola::query()->delete();
            UnitSpam::query()->delete();

            // 3. Import Unit SPAM
            $this->info("Importing SPAM units...");
            $unitSpamStmt = $sqlite->query("SELECT * FROM unit_spam");
            $sqliteUnits = $unitSpamStmt->fetchAll(\PDO::FETCH_ASSOC);

            $spamUnitMap = []; // Map sqlite_unit_id => mysql_unit_id
            $importedUnits = 0;

            foreach ($sqliteUnits as $sUnit) {
                $mysqlDesaId = $sqliteToMySqlDesaMap[$sUnit['desa_id']] ?? null;
                
                if (!$mysqlDesaId) {
                    $this->warn("Skipping SQLite Unit SPAM ID {$sUnit['id']} - corresponding Desa not found in MySQL.");
                    continue;
                }

                $newUnit = UnitSpam::create([
                    'desa_id' => $mysqlDesaId,
                    'name' => $sUnit['name'] ?: null,
                    'is_simspam' => (bool)$sUnit['is_simspam'],
                    'sistem_layanan' => $sUnit['sistem_layanan'] ?: null,
                    'sumber_mata_air_kap' => $sUnit['sumber_mata_air_kap'] ?: null,
                    'sumber_air_tanah_kap' => $sUnit['sumber_air_tanah_kap'] ?: null,
                    'lain_lain_kap' => $sUnit['lain_lain_kap'] ?: null,
                    'sumber_dana' => $sUnit['sumber_dana'] ?: null,
                    'program' => $sUnit['program'] ?: null,
                    'tarif_dasar_hukum' => $sUnit['tarif_dasar_hukum'] ?: null,
                    'iuran_nominal' => $sUnit['iuran_nominal'] ?: null,
                    'biaya_operasional' => $sUnit['biaya_operasional'] ?: null,
                    'created_at' => $sUnit['created_at'] ?: now(),
                    'updated_at' => $sUnit['updated_at'] ?: now(),
                ]);

                $spamUnitMap[$sUnit['id']] = $newUnit->id;
                $importedUnits++;
            }

            $this->info("Imported {$importedUnits} SPAM units successfully.");

            // 4. Import Pengelola
            $this->info("Importing POKMAS managers...");
            $pengelolaStmt = $sqlite->query("SELECT * FROM pengelola");
            $sqlitePengelola = $pengelolaStmt->fetchAll(\PDO::FETCH_ASSOC);
            $importedPengelola = 0;

            foreach ($sqlitePengelola as $sPengelola) {
                $mysqlUnitId = $spamUnitMap[$sPengelola['unit_spam_id']] ?? null;

                if (!$mysqlUnitId) continue;

                Pengelola::create([
                    'unit_spam_id' => $mysqlUnitId,
                    'pokmas' => $sPengelola['pokmas'] ?: null,
                    'perdes' => $sPengelola['perdes'] ?: null,
                    'kepala' => $sPengelola['kepala'] ?: null,
                    'bendahara' => $sPengelola['bendahara'] ?: null,
                    'sekretaris' => $sPengelola['sekretaris'] ?: null,
                ]);
                $importedPengelola++;
            }

            $this->info("Imported {$importedPengelola} POKMAS managers successfully.");

            // 5. Import Achievements
            $this->info("Importing connection achievements...");
            $achievementStmt = $sqlite->query("SELECT * FROM achievements");
            $sqliteAchievements = $achievementStmt->fetchAll(\PDO::FETCH_ASSOC);
            $importedAchievements = 0;

            foreach ($sqliteAchievements as $sAch) {
                $mysqlUnitId = $spamUnitMap[$sAch['unit_spam_id']] ?? null;

                if (!$mysqlUnitId) continue;

                SpamAchievement::create([
                    'unit_spam_id' => $mysqlUnitId,
                    'tahun' => $sAch['tahun'],
                    'jumlah_sr' => $sAch['jumlah_sr'] ?? 0,
                    'jumlah_kk' => $sAch['jumlah_kk'] ?? 0,
                    'jumlah_jiwa' => $sAch['jumlah_jiwa'] ?? 0,
                    'jumlah_bjp_kk' => $sAch['jumlah_bjp_kk'] ?? 0,
                    'jumlah_bjp_jiwa' => $sAch['jumlah_bjp_jiwa'] ?? 0,
                    'catatan' => $sAch['catatan'] ?: null,
                ]);
                $importedAchievements++;
            }

            $this->info("Imported {$importedAchievements} achievements successfully.");

            // 6. Import Unit Checklists
            $this->info("Importing unit checklists...");
            $checklistStmt = $sqlite->query("SELECT * FROM unit_checklists");
            $sqliteChecklists = $checklistStmt->fetchAll(\PDO::FETCH_ASSOC);
            $importedChecklists = 0;

            foreach ($sqliteChecklists as $sCheck) {
                $mysqlUnitId = $spamUnitMap[$sCheck['unit_spam_id']] ?? null;

                if (!$mysqlUnitId) continue;

                UnitChecklist::create([
                    'unit_spam_id' => $mysqlUnitId,
                    'item' => $sCheck['item'],
                    'is_checked' => (bool)$sCheck['is_checked'],
                ]);
                $importedChecklists++;
            }

            $this->info("Imported {$importedChecklists} checklists successfully.");

            DB::commit();
            $this->info("Consolidation transaction committed successfully!");
            return 0;

        } catch (\Exception $e) {
            DB::rollBack();
            $this->error("Consolidation failed! Transaction rolled back.");
            $this->error($e->getMessage());
            return 1;
        }
    }

    private function normalize(?string $value): string
    {
        if (!$value) {
            return '';
        }
        $value = str_ireplace(['kecamatan', 'desa', 'kelurahan'], '', $value);
        $normalized = strtolower(preg_replace('/[^a-z0-9]+/i', '', $value) ?? '');
        
        // Manual spelling alias mapping for 100% compatibility
        if ($normalized === 'salagedang') {
            return 'selagedang';
        }
        
        return $normalized;
    }
}
