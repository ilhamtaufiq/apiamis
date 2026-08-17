<?php

namespace App\Http\Controllers;

use App\Http\Resources\PuspenMediaShareResource;
use App\Models\PuspenMediaShare;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Str;
use Spatie\MediaLibrary\MediaCollections\Models\Media;
use Symfony\Component\HttpFoundation\BinaryFileResponse;
use ZipArchive;

class PuspenMediaShareController extends Controller
{
    public function index(Request $request)
    {
        $validated = $request->validate([
            'search' => 'nullable|string|max:100',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:100',
        ]);

        $query = PuspenMediaShare::ownedBy($request->user()->id)
            ->with('media')
            ->orderByDesc('created_at');

        if (! empty($validated['search'] ?? null)) {
            $search = $validated['search'];
            $query->where(function ($builder) use ($search) {
                $builder->where('title', 'like', "%{$search}%")
                    ->orWhere('description', 'like', "%{$search}%");
            });
        }

        $perPage = (int) ($validated['per_page'] ?? 10);

        return PuspenMediaShareResource::collection($query->paginate($perPage));
    }

    public function mediaLibrary(Request $request): JsonResponse
    {
        $query = Media::query()->orderByDesc('created_at');

        if ($request->filled('search')) {
            $search = $request->search;
            $query->where(function ($builder) use ($search) {
                $builder->where('name', 'like', "%{$search}%")
                    ->orWhere('file_name', 'like', "%{$search}%")
                    ->orWhere('collection_name', 'like', "%{$search}%");
            });
        }

        if ($request->filled('mime_group') && $request->mime_group !== 'all') {
            match ($request->mime_group) {
                'image' => $query->where('mime_type', 'like', 'image/%'),
                'video' => $query->where('mime_type', 'like', 'video/%'),
                'document' => $query->where(function ($builder) {
                    $builder->where('mime_type', 'like', 'application/%')
                        ->orWhere('mime_type', 'like', 'text/%');
                }),
                default => null,
            };
        }

        $items = $query->limit((int) $request->get('limit', 50))->get();

        return response()->json([
            'data' => $items->map(fn (Media $media) => [
                'id' => (string) $media->id,
                'name' => $media->name,
                'file_name' => $media->file_name,
                'mime_type' => $media->mime_type,
                'size' => $media->size,
                'url' => $media->getFullUrl(),
                'collection_name' => $media->collection_name,
                'model_type' => class_basename($media->model_type),
                'created_at' => $media->created_at?->toISOString(),
            ])->values(),
        ]);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'title' => 'required|string|max:255',
            'description' => 'nullable|string|max:2000',
            'is_public' => 'nullable|boolean',
            'expires_at' => 'nullable|date',
            'file' => 'required_without_all:media_id,files,media_ids|file|max:204800',
            'files' => 'required_without_all:file,media_id,media_ids|array',
            'files.*' => 'file|max:204800',
            'file_folders' => 'nullable|array',
            'file_folders.*' => 'nullable|string|max:255',
            'media_id' => 'required_without_all:file,files,media_ids|nullable|exists:media,id',
            'media_ids' => 'required_without_all:file,files,media_id|array',
            'media_ids.*' => 'integer|exists:media,id',
            'media_folders' => 'nullable|array',
            'media_folders.*' => 'nullable|string|max:255',
        ]);

        $share = DB::transaction(function () use ($request, $validated) {
            $share = PuspenMediaShare::create([
                'user_id' => $request->user()->id,
                'title' => $validated['title'],
                'description' => $validated['description'] ?? null,
                'share_token' => $this->makeShareToken(),
                'is_public' => $request->boolean('is_public', true),
                'expires_at' => $validated['expires_at'] ?? null,
            ]);

            $this->attachMedia($share, $request);

            return $share->load('media');
        });

        return (new PuspenMediaShareResource($share))
            ->response()
            ->setStatusCode(201);
    }

    public function update(Request $request, PuspenMediaShare $puspenMediaShare)
    {
        abort_unless($puspenMediaShare->canManage($request->user()), 403, 'Anda tidak memiliki akses untuk mengubah media sharing ini');

        $validated = $request->validate([
            'title' => 'sometimes|required|string|max:255',
            'description' => 'nullable|string|max:2000',
            'is_public' => 'nullable|boolean',
            'expires_at' => 'nullable|date',
            'file' => 'nullable|file|max:204800',
            'files' => 'nullable|array',
            'files.*' => 'file|max:204800',
            'file_folders' => 'nullable|array',
            'file_folders.*' => 'nullable|string|max:255',
            'media_id' => 'nullable|exists:media,id',
            'media_ids' => 'nullable|array',
            'media_ids.*' => 'integer|exists:media,id',
            'media_folders' => 'nullable|array',
            'media_folders.*' => 'nullable|string|max:255',
        ]);

        $share = DB::transaction(function () use ($request, $validated, $puspenMediaShare) {
            $puspenMediaShare->update([
                ...collect($validated)->only(['title', 'description', 'expires_at'])->all(),
                'is_public' => $request->has('is_public') ? $request->boolean('is_public') : $puspenMediaShare->is_public,
            ]);

            if ($this->hasMediaPayload($request)) {
                $puspenMediaShare->clearMediaCollection('shared-media');
                $this->attachMedia($puspenMediaShare, $request);
            }

            return $puspenMediaShare->load('media');
        });

        return new PuspenMediaShareResource($share);
    }

    public function destroy(Request $request, PuspenMediaShare $puspenMediaShare): JsonResponse
    {
        abort_unless($puspenMediaShare->canManage($request->user()), 403, 'Anda tidak memiliki akses untuk menghapus media sharing ini');

        $puspenMediaShare->delete();

        return response()->json([
            'success' => true,
            'message' => 'Media sharing berhasil dihapus',
        ]);
    }

    public function publicShow(string $shareToken)
    {
        $share = PuspenMediaShare::where('share_token', $shareToken)
            ->with('media')
            ->firstOrFail();

        abort_unless($share->isDownloadable(), 404, 'Media sharing tidak tersedia');

        return new PuspenMediaShareResource($share);
    }

    public function publicPreview(string $shareToken, Media $media)
    {
        $share = PuspenMediaShare::where('share_token', $shareToken)
            ->with('media')
            ->firstOrFail();

        abort_unless($share->isDownloadable(), 404, 'Media sharing tidak tersedia');
        abort_unless(
            $share->getMedia('shared-media')->contains(fn (Media $item) => $item->id === $media->id),
            404,
            'File tidak ditemukan'
        );
        abort_unless(file_exists($media->getPath()), 404, 'File tidak ditemukan');

        return response(file_get_contents($media->getPath()), 200, [
            'Content-Type' => $media->mime_type ?: 'application/octet-stream',
            'Content-Disposition' => 'inline; filename="'.$media->file_name.'"',
            'Cache-Control' => 'public, max-age=3600',
        ]);
    }

    public function publicDownload(string $shareToken): BinaryFileResponse
    {
        $share = PuspenMediaShare::where('share_token', $shareToken)
            ->with('media')
            ->firstOrFail();

        abort_unless($share->isDownloadable(), 404, 'Media sharing tidak tersedia');

        $mediaItems = $share->getMedia('shared-media')
            ->filter(fn (Media $media) => file_exists($media->getPath()))
            ->values();

        abort_unless($mediaItems->isNotEmpty(), 404, 'File tidak ditemukan');

        $share->forceFill([
            'download_count' => $share->download_count + 1,
            'last_downloaded_at' => now(),
        ])->save();

        if ($mediaItems->count() === 1) {
            $media = $mediaItems->first();

            return response()->download($media->getPath(), $media->file_name, [
                'Content-Type' => $media->mime_type ?: 'application/octet-stream',
            ]);
        }

        $zipPath = tempnam(sys_get_temp_dir(), 'puspen-share-');
        $zip = new ZipArchive();
        abort_unless($zip->open($zipPath, ZipArchive::OVERWRITE) === true, 500, 'Gagal membuat arsip download');

        $usedNames = [];
        foreach ($mediaItems as $media) {
            $folderPath = $this->cleanFolderPath((string) $media->getCustomProperty('folder_path'));
            $archiveName = $this->uniqueArchiveName($media->file_name, $usedNames, $folderPath);
            $zip->addFile($media->getPath(), $archiveName);
        }
        $zip->close();

        return response()->download($zipPath, Str::slug($share->title).'-media-sharing.zip', [
            'Content-Type' => 'application/zip',
        ])->deleteFileAfterSend(true);
    }

    private function attachMedia(PuspenMediaShare $share, Request $request): void
    {
        if ($request->hasFile('files')) {
            foreach ($request->file('files') as $index => $file) {
                $folderPath = $this->cleanFolderPath((string) data_get($request->input('file_folders', []), $index));
                $share->addMedia($file)
                    ->withCustomProperties(array_filter([
                        'folder_path' => $folderPath,
                    ]))
                    ->toMediaCollection('shared-media');
            }

            return;
        }

        if ($request->hasFile('file')) {
            $folderPath = $this->cleanFolderPath((string) data_get($request->input('file_folders', []), 0));
            $share->addMediaFromRequest('file')
                ->withCustomProperties(array_filter([
                    'folder_path' => $folderPath,
                ]))
                ->toMediaCollection('shared-media');

            return;
        }

        if ($request->filled('media_ids')) {
            foreach ($request->input('media_ids', []) as $index => $mediaId) {
                $folderPath = $this->cleanFolderPath((string) data_get($request->input('media_folders', []), $index));
                $this->copyLibraryMedia($share, (int) $mediaId, $folderPath);
            }

            return;
        }

        if ($request->filled('media_id')) {
            $folderPath = $this->cleanFolderPath((string) data_get($request->input('media_folders', []), 0));
            $this->copyLibraryMedia($share, $request->integer('media_id'), $folderPath);
        }
    }

    private function hasMediaPayload(Request $request): bool
    {
        return $request->hasFile('file')
            || $request->hasFile('files')
            || $request->filled('media_id')
            || $request->filled('media_ids');
    }

    private function copyLibraryMedia(PuspenMediaShare $share, int $mediaId, string $folderPath = ''): void
    {
        $media = Media::findOrFail($mediaId);
        abort_unless(file_exists($media->getPath()), 404, 'File media library tidak ditemukan');

        $share->addMedia($media->getPath())
            ->preservingOriginal()
            ->usingName($media->name)
            ->usingFileName($media->file_name)
            ->withCustomProperties([
                'source_media_id' => $media->id,
                'source_model_type' => $media->model_type,
                'source_model_id' => $media->model_id,
                'folder_path' => $folderPath,
            ])
            ->toMediaCollection('shared-media');
    }

    private function uniqueArchiveName(string $fileName, array &$usedNames, string $folderPath = ''): string
    {
        $pathInfo = pathinfo($fileName);
        $baseName = $pathInfo['filename'] ?: 'file';
        $extension = isset($pathInfo['extension']) ? '.'.$pathInfo['extension'] : '';
        $archiveName = ($folderPath ? "{$folderPath}/" : '').$baseName.$extension;
        $index = 2;

        while (isset($usedNames[$archiveName])) {
            $archiveName = ($folderPath ? "{$folderPath}/" : '')."{$baseName}-{$index}{$extension}";
            $index++;
        }

        $usedNames[$archiveName] = true;

        return $archiveName;
    }

    private function cleanFolderPath(string $folderPath): string
    {
        $segments = collect(explode('/', str_replace('\\', '/', $folderPath)))
            ->map(fn ($segment) => trim($segment))
            ->filter(fn ($segment) => $segment !== '' && $segment !== '.' && $segment !== '..')
            ->map(fn ($segment) => Str::slug($segment, '-'))
            ->filter()
            ->values();

        return $segments->implode('/');
    }

    private function makeShareToken(): string
    {
        do {
            $token = Str::random(32);
        } while (PuspenMediaShare::where('share_token', $token)->exists());

        return $token;
    }

    public function destroyMedia(Request $request): JsonResponse
    {
        abort_unless($request->user()->hasRole('admin'), 403, 'Hanya admin yang dapat menghapus media');

        $validated = $request->validate([
            'ids' => 'required|array|min:1',
            'ids.*' => 'integer',
        ]);

        $media = Media::whereIn('id', $validated['ids'])->get();

        if ($media->isEmpty()) {
            return response()->json(['message' => 'Media tidak ditemukan'], 404);
        }

        $deleted = DB::transaction(function () use ($media) {
            $count = 0;
            foreach ($media as $item) {
                $item->delete();
                $count++;
            }

            return $count;
        });

        return response()->json([
            'success' => true,
            'message' => "{$deleted} media dihapus",
            'deleted' => $deleted,
        ]);
    }
}
