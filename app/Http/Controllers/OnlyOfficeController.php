<?php

namespace App\Http\Controllers;

use App\Services\OnlyOffice\OnlyOfficeJwt;
use App\Services\OnlyOffice\OnlyOfficeMediaAuthorizer;
use App\Services\OnlyOffice\OnlyOfficeService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Http;
use Illuminate\Support\Facades\Log;
use Spatie\MediaLibrary\MediaCollections\Models\Media;
use Symfony\Component\HttpFoundation\BinaryFileResponse;

class OnlyOfficeController extends Controller
{
    public function __construct(
        private readonly OnlyOfficeService $onlyOffice,
        private readonly OnlyOfficeMediaAuthorizer $authorizer,
    ) {}

    public function config(Request $request, Media $media): JsonResponse
    {
        if (! $this->onlyOffice->isEnabled()) {
            return response()->json([
                'message' => 'ONLYOFFICE Document Server belum dikonfigurasi.',
            ], 503);
        }

        if (! $this->onlyOffice->supportsMedia($media)) {
            return response()->json([
                'message' => 'Format file tidak didukung ONLYOFFICE.',
            ], 422);
        }

        return response()->json([
            'data' => $this->onlyOffice->buildEditorPayload($request->user(), $media),
        ]);
    }

    public function download(Request $request, Media $media): BinaryFileResponse
    {
        abort_unless($request->hasValidSignature(), 403, 'Tautan unduhan tidak valid atau kedaluwarsa.');
        abort_unless(file_exists($media->getPath()), 404, 'File tidak ditemukan.');

        return response()->file($media->getPath(), [
            'Content-Type' => $media->mime_type ?: 'application/octet-stream',
            'Content-Disposition' => 'inline; filename="'.$media->file_name.'"',
            'Cache-Control' => 'private, max-age=60',
        ]);
    }

    public function callback(Request $request): JsonResponse
    {
        $payload = $request->all();
        $jwtSecret = (string) config('onlyoffice.jwt_secret');

        if ($jwtSecret !== '') {
            $token = $request->input('token')
                ?? $request->bearerToken()
                ?? $this->extractJwtFromBody($payload);

            $decoded = is_string($token) ? OnlyOfficeJwt::decode($token, $jwtSecret) : null;
            if (is_array($decoded)) {
                $payload = array_merge($payload, $decoded);
            }
        }

        $status = (int) ($payload['status'] ?? 0);
        if (! in_array($status, [2, 6], true)) {
            return response()->json(['error' => 0]);
        }

        $downloadUrl = $payload['url'] ?? null;
        $documentKey = (string) ($payload['key'] ?? '');

        if (! $downloadUrl || $documentKey === '') {
            Log::warning('ONLYOFFICE callback missing url/key', ['payload' => $payload]);

            return response()->json(['error' => 1]);
        }

        $mediaId = $this->onlyOffice->parseDocumentKey($documentKey);
        if (! $mediaId) {
            return response()->json(['error' => 1]);
        }

        $media = Media::query()->find($mediaId);
        if (! $media || ! $media->model) {
            return response()->json(['error' => 1]);
        }

        try {
            $response = Http::timeout(120)->get($downloadUrl);
            abort_unless($response->successful(), 500, 'Gagal mengunduh dokumen dari ONLYOFFICE.');

            $tempPath = tempnam(sys_get_temp_dir(), 'onlyoffice-save-');
            file_put_contents($tempPath, $response->body());

            $owner = $media->model;
            $collection = $media->collection_name;
            $fileName = $media->file_name;
            $customProperties = $media->custom_properties;

            $media->delete();

            $owner->addMedia($tempPath)
                ->usingFileName($fileName)
                ->withCustomProperties($customProperties)
                ->toMediaCollection($collection);

            @unlink($tempPath);
        } catch (\Throwable $exception) {
            Log::error('ONLYOFFICE callback save failed', [
                'media_id' => $mediaId,
                'message' => $exception->getMessage(),
            ]);

            return response()->json(['error' => 1]);
        }

        return response()->json(['error' => 0]);
    }

    private function extractJwtFromBody(array $payload): ?string
    {
        foreach (['token', 'payload', 'body'] as $key) {
            if (isset($payload[$key]) && is_string($payload[$key])) {
                return $payload[$key];
            }
        }

        return null;
    }
}