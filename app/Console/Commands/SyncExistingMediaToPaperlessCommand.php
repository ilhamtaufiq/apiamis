<?php

namespace App\Console\Commands;

use App\Jobs\SyncMediaToPaperlessJob;
use Illuminate\Console\Command;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class SyncExistingMediaToPaperlessCommand extends Command
{
    protected $signature = 'paperless:sync-existing
                            {--model= : Filter by model_type class}
                            {--collection= : Filter by collection_name}
                            {--chunk=100 : Chunk size for processing}';

    protected $description = 'Dispatch sync jobs to Paperless-ngx for existing Spatie media records';

    public function handle(): int
    {
        $query = Media::query();

        if ($model = $this->option('model')) {
            $query->where('model_type', $model);
        }

        if ($collection = $this->option('collection')) {
            $query->where('collection_name', $collection);
        }

        $allowedMimes = (array) config('paperless.allowed_mimes', []);
        if (!empty($allowedMimes)) {
            $query->whereIn('mime_type', $allowedMimes);
        }

        $total = $query->count();
        if ($total === 0) {
            $this->info('No matching media records found to sync.');
            return Command::SUCCESS;
        }

        $this->info("Found {$total} media items to sync.");

        $count = 0;
        $query->chunk((int) $this->option('chunk'), function ($items) use (&$count) {
            foreach ($items as $media) {
                if ($media->hasCustomProperty('paperless_id') || $media->hasCustomProperty('paperless_task_id')) {
                    continue;
                }

                SyncMediaToPaperlessJob::dispatch($media)
                    ->onQueue((string) config('paperless.queue', 'default'));
                $count++;
            }
        });

        $this->info("Dispatched {$count} sync jobs to queue.");

        return Command::SUCCESS;
    }
}
