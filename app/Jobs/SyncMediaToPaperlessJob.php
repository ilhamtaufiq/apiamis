<?php

namespace App\Jobs;

use App\Services\PaperlessService;
use Illuminate\Bus\Queueable;
use Illuminate\Contracts\Queue\ShouldQueue;
use Illuminate\Foundation\Bus\Dispatchable;
use Illuminate\Queue\InteractsWithQueue;
use Illuminate\Queue\SerializesModels;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class SyncMediaToPaperlessJob implements ShouldQueue
{
    use Dispatchable, InteractsWithQueue, Queueable, SerializesModels;

    public int $tries = 3;
    public int $backoff = 60;

    public function __construct(public Media $media)
    {
    }

    public function handle(PaperlessService $paperless): void
    {
        if ($this->media->hasCustomProperty('paperless_id') || $this->media->hasCustomProperty('paperless_task_id')) {
            return;
        }

        $allowedMimes = (array) config('paperless.allowed_mimes', []);
        if (!in_array($this->media->mime_type, $allowedMimes, true)) {
            return;
        }

        $filePath = $this->media->getPath();
        if (!file_exists($filePath)) {
            return;
        }

        $response = $paperless->uploadDocument(
            $filePath,
            $this->media->file_name,
            [
                'title' => $this->media->name ?: $this->media->file_name,
                'tags' => array_filter([$this->media->model_type, $this->media->collection_name]),
            ]
        );

        if ($response->successful()) {
            $taskUuid = $response->json();
            $this->media->setCustomProperty('paperless_task_id', is_string($taskUuid) ? $taskUuid : json_encode($taskUuid));
            $this->media->save();
        }
    }
}
