<?php

namespace App\Console\Commands;

use App\Services\SpmAirMinumConsolidationService;
use Illuminate\Console\Command;

class ConsolidateSpmAirMinum extends Command
{
    protected $signature = 'spm-air-minum:consolidate';

    protected $description = 'Konsolidasi SPM Air Minum JP/BJP per desa dari raw SPAM';

    public function handle(SpmAirMinumConsolidationService $service): int
    {
        $result = $service->consolidate();

        $this->info("Konsolidasi selesai. {$result['consolidated']} desa diproses.");
        $this->line("Matched: {$result['matched']}, ambiguous: {$result['ambiguous']}, unmatched: {$result['unmatched']}");

        return self::SUCCESS;
    }
}
