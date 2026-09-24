<?php

namespace App\Http\Controllers;

use App\Jobs\SyncMediaToPaperlessJob;
use App\Services\PaperlessService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Symfony\Component\HttpFoundation\Response;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class PaperlessController extends Controller
{
    public function sync(Media $media): JsonResponse
    {
        SyncMediaToPaperlessJob::dispatch($media);

        return response()->json([
            'message' => 'Paperless sync job queued',
            'media_id' => $media->id,
        ]);
    }

    public function syncAll(Request $request): JsonResponse
    {
        $model = $request->input('model_type');
        $collection = $request->input('collection_name');

        $query = Media::query();

        if ($model) {
            $query->where('model_type', $model);
        }

        if ($collection) {
            $query->where('collection_name', $collection);
        }

        $allowedMimes = (array) config('paperless.allowed_mimes', []);
        if (!empty($allowedMimes)) {
            $query->whereIn('mime_type', $allowedMimes);
        }

        $dispatched = 0;
        $query->chunk(100, function ($items) use (&$dispatched) {
            foreach ($items as $media) {
                if ($media->hasCustomProperty('paperless_id') || $media->hasCustomProperty('paperless_task_id')) {
                    continue;
                }

                SyncMediaToPaperlessJob::dispatch($media)
                    ->onQueue((string) config('paperless.queue', 'default'));
                $dispatched++;
            }
        });

        return response()->json([
            'message' => "Dispatched {$dispatched} media sync jobs to queue",
            'dispatched_count' => $dispatched,
        ]);
    }

    public function show(Media $media, PaperlessService $service): JsonResponse
    {
        $paperlessId = $media->getCustomProperty('paperless_id');
        if (!$paperlessId) {
            return response()->json(['error' => 'Media not synced to Paperless yet'], Response::HTTP_NOT_FOUND);
        }

        $res = $service->getDocument((int) $paperlessId);

        return response()->json($res->json(), $res->status());
    }

    public function download(Media $media, PaperlessService $service)
    {
        $paperlessId = $media->getCustomProperty('paperless_id');
        if (!$paperlessId) {
            return response()->json(['error' => 'Media not synced to Paperless yet'], Response::HTTP_NOT_FOUND);
        }

        $res = $service->downloadDocument((int) $paperlessId);

        return response($res->body(), $res->status(), [
            'Content-Type' => $res->header('Content-Type') ?: 'application/octet-stream',
            'Content-Disposition' => $res->header('Content-Disposition') ?: 'attachment; filename="' . $media->file_name . '"',
        ]);
    }

    public function search(Request $request, PaperlessService $service): JsonResponse
    {
        $query = (string) $request->query('query', '');
        $page = (int) $request->query('page', 1);

        $res = $service->searchDocuments($query, $page);

        return response()->json($res->json(), $res->status());
    }
}
