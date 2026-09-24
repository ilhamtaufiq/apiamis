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

    /**
     * Batch status: media_id mana saja yang sudah tersinkron.
     * Satu request pengganti N query status per kartu (hindari N+1 di UI).
     */
    public function syncedIds(Request $request): JsonResponse
    {
        $ids = collect($request->input('media_ids', []))
            ->map(fn($id) => (int) $id)
            ->filter(fn($id) => $id > 0)
            ->unique()
            ->take(500)
            ->values();

        if ($ids->isEmpty()) {
            return response()->json(['synced_ids' => []]);
        }

        $synced = Media::query()
            ->whereIn('id', $ids)
            ->whereNotNull('custom_properties->paperless_id')
            ->pluck('id')
            ->map(fn($id) => (int) $id)
            ->values();

        return response()->json(['synced_ids' => $synced]);
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

    /**
     * Cek dua arah (read-only, tanpa write):
     * Arah 1 Laravel -> Paperless: status task pending + verifikasi dokumen paperless_id.
     * Arah 2 Paperless -> Laravel: dokumen Paperless yang tidak tertaut ke media mana pun.
     */
    public function reconcile(Request $request, PaperlessService $service): JsonResponse
    {
        $limit = max(1, min((int) $request->query('limit', 50), 200));

        $pending = Media::query()
            ->whereNotNull('custom_properties->paperless_task_id')
            ->whereNull('custom_properties->paperless_id')
            ->take($limit)
            ->get(['id', 'file_name', 'custom_properties']);

        $pendingStatus = [];
        foreach ($pending as $media) {
            $taskUuid = (string) $media->getCustomProperty('paperless_task_id');
            $state = $this->resolveTaskState($service, $taskUuid);
            $pendingStatus[] = [
                'media_id' => $media->id,
                'file_name' => $media->file_name,
                'task_id' => $taskUuid,
                'state' => $state['state'],
                'paperless_id' => $state['paperless_id'],
            ];
        }

        $linked = Media::query()
            ->whereNotNull('custom_properties->paperless_id')
            ->take($limit)
            ->get(['id', 'custom_properties']);

        $verified = ['ok' => [], 'missing' => []];
        $linkedIds = [];
        foreach ($linked as $media) {
            $docId = (int) $media->getCustomProperty('paperless_id');
            $linkedIds[] = $docId;
            $res = $service->getDocument($docId);
            if ($res->status() === Response::HTTP_NOT_FOUND) {
                $verified['missing'][] = ['media_id' => $media->id, 'paperless_id' => $docId];
            } elseif ($res->successful()) {
                $verified['ok'][] = $media->id;
            }
        }

        $paperlessIds = $this->collectPaperlessIds($service, 200);
        $orphans = array_values(array_diff($paperlessIds, $linkedIds));

        return response()->json([
            'summary' => [
                'unsynced_count' => Media::query()
                    ->whereNull('custom_properties->paperless_task_id')
                    ->whereNull('custom_properties->paperless_id')
                    ->count(),
                'pending_count' => Media::query()
                    ->whereNotNull('custom_properties->paperless_task_id')
                    ->whereNull('custom_properties->paperless_id')
                    ->count(),
                'linked_count' => Media::query()
                    ->whereNotNull('custom_properties->paperless_id')
                    ->count(),
                'paperless_total' => $this->paperlessTotal($service),
                'orphan_count' => count($orphans),
            ],
            'pending' => $pendingStatus,
            'verified' => $verified,
            'orphan_paperless_ids' => $orphans,
        ]);
    }

    /**
     * @return array{state: string, paperless_id: int|null}
     */
    private function resolveTaskState(PaperlessService $service, string $taskUuid): array
    {
        if ($taskUuid === '') {
            return ['state' => 'stale', 'paperless_id' => null];
        }

        $res = $service->getTaskByUuid($taskUuid);
        if ($res->failed()) {
            return ['state' => 'unknown', 'paperless_id' => null];
        }

        $task = $res->json('results.0');
        if (!is_array($task)) {
            return ['state' => 'stale', 'paperless_id' => null];
        }

        $status = (string) ($task['status'] ?? '');
        if (in_array($status, ['pending', 'started'], true)) {
            return ['state' => 'processing', 'paperless_id' => null];
        }

        if ($status === 'success') {
            $docId = $task['related_document_ids'][0] ?? $task['result_data']['document_id'] ?? null;

            return ['state' => 'synced', 'paperless_id' => is_numeric($docId) ? (int) $docId : null];
        }

        return ['state' => 'failed', 'paperless_id' => null];
    }

    /** @return int[] */
    private function collectPaperlessIds(PaperlessService $service, int $cap): array
    {
        $ids = [];
        $page = 1;
        while (count($ids) < $cap) {
            $res = $service->listDocuments($page, 100);
            if ($res->failed()) {
                break;
            }
            $results = $res->json('results', []);
            if (!is_array($results) || $results === []) {
                break;
            }
            foreach ($results as $doc) {
                if (isset($doc['id'])) {
                    $ids[] = (int) $doc['id'];
                }
            }
            if ($res->json('next') === null) {
                break;
            }
            $page++;
        }

        return array_values(array_unique(array_slice($ids, 0, $cap)));
    }

    private function paperlessTotal(PaperlessService $service): ?int
    {
        $res = $service->listDocuments(1, 1);
        if ($res->failed()) {
            return null;
        }

        $count = $res->json('count');

        return is_numeric($count) ? (int) $count : null;
    }
}
