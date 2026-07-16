<?php

namespace App\Http\Controllers;

use App\Services\OnlyOffice\OnlyOfficeDownloadToken;
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

    public function health(): JsonResponse
    {
        $enabled = $this->onlyOffice->isEnabled();
        $documentServerUrl = rtrim((string) config('onlyoffice.document_server_url'), '/');
        $reachable = false;
        $message = $enabled ? 'Document Server dikonfigurasi.' : 'ONLYOFFICE belum dikonfigurasi.';

        if ($enabled && $documentServerUrl !== '') {
            try {
                $response = Http::timeout(5)->get($documentServerUrl.'/healthcheck');
                $reachable = $response->successful()
                    || str_contains(strtolower($response->body()), 'true');
                $message = $reachable
                    ? 'Document Server siap.'
                    : 'Document Server tidak merespons healthcheck.';
            } catch (\Throwable $exception) {
                $message = 'Document Server tidak terjangkau: '.$exception->getMessage();
            }
        }

        return response()->json([
            'data' => [
                'enabled' => $enabled,
                'reachable' => $reachable,
                'document_server_url' => $documentServerUrl !== '' ? $documentServerUrl.'/' : null,
                'message' => $message,
            ],
        ], $enabled && $reachable ? 200 : 503);
    }

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

        $mode = $request->query('mode');
        if (is_string($mode)) {
            $mode = strtolower($mode);
        } else {
            $mode = null;
        }

        if ($mode !== null && ! in_array($mode, ['view', 'edit'], true)) {
            return response()->json(['message' => 'Mode tidak valid. Gunakan view atau edit.'], 422);
        }

        return response()->json([
            'data' => $this->onlyOffice->buildEditorPayload($request->user(), $media, $mode),
        ]);
    }

    public function download(Request $request, Media $media): BinaryFileResponse
    {
        $expiresAt = (int) $request->query('expires', 0);
        $token = (string) $request->query('token', '');

        $hasOnlyOfficeToken = OnlyOfficeDownloadToken::valid($media->id, $expiresAt, $token);
        $hasLegacySignature = $request->hasValidSignature();
        $hasUserAccess = $request->user() && $this->authorizer->canAccess($request->user(), $media);

        abort_unless(
            $hasOnlyOfficeToken || $hasLegacySignature || $hasUserAccess,
            403,
            'Tautan unduhan tidak valid atau kedaluwarsa.',
        );
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
        // 2 = ready for saving, 6 = force save
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

            // Keep the same media_id so links and keys remain stable.
            $this->onlyOffice->overwriteMediaFile($media, $response->body());
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
