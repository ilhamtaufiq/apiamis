<?php

namespace App\Listeners;

use App\Jobs\SyncMediaToPaperlessJob;
use Spatie\MediaLibrary\MediaCollections\Events\MediaHasBeenAdded;

class SendMediaToPaperlessListener
{
    public function handle(MediaHasBeenAdded $event): void
    {
        if (!config('paperless.auto_sync')) {
            return;
        }

        SyncMediaToPaperlessJob::dispatch($event->media)
            ->onQueue((string) config('paperless.queue', 'default'));
    }
}
