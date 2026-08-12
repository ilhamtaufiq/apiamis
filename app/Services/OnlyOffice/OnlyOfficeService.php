<?php

namespace App\Services\OnlyOffice;

use App\Models\User;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class OnlyOfficeService
{
    private const SUPPORTED_EXTENSIONS = [
        'doc', 'docx', 'odt', 'rtf', 'txt',
        'xls', 'xlsx', 'ods', 'csv',
        'ppt', 'pptx', 'odp',
        'pdf',
    ];

    public function __construct(
        private readonly OnlyOfficeMediaAuthorizer $authorizer,
    ) {}

    public function isEnabled(): bool
    {
        return (bool) config('onlyoffice.document_server_url');
    }

    public function supportsMedia(Media $media): bool
    {
        return $this->supportsExtension($media->extension ?: pathinfo($media->file_name, PATHINFO_EXTENSION));
    }

    public function supportsExtension(string $extension): bool
    {
        return in_array(strtolower($extension), self::SUPPORTED_EXTENSIONS, true);
    }

    /**
     * @param  'view'|'edit'|null  $requestedMode
     */
    public function buildEditorPayload(User $user, Media $media, ?string $requestedMode = null): array
    {
        abort_unless($this->isEnabled(), 503, 'ONLYOFFICE Document Server belum dikonfigurasi.');
        abort_unless($this->supportsMedia($media), 422, 'Format file tidak didukung ONLYOFFICE.');
        abort_unless($this->authorizer->canAccess($user, $media), 403, 'Anda tidak memiliki akses ke dokumen ini.');

        $canEdit = $this->authorizer->canEdit($user, $media);
        $extension = strtolower($media->extension ?: pathinfo($media->file_name, PATHINFO_EXTENSION));

        // PDF is view-only in practice for most deployments.
        if ($extension === 'pdf') {
            $canEdit = false;
        }

        $mode = $this->resolveMode($requestedMode, $canEdit);
        $isEdit = $mode === 'edit';
        $documentKey = $this->buildDocumentKey($media);
        $downloadUrl = OnlyOfficeDownloadToken::buildDownloadUrl($media->id);

        $callbackUrl = $this->resolveCallbackUrl();

        $config = [
            'document' => [
                'fileType' => $extension,
                'key' => $documentKey,
                'title' => $media->file_name,
                'url' => $downloadUrl,
                'permissions' => [
                    'edit' => $isEdit,
                    'download' => true,
                    'print' => true,
                    'review' => $isEdit,
                    'comment' => $isEdit,
                    'fillForms' => $isEdit,
                    'editCommentAuthorOnly' => false,
                ],
            ],
            'documentType' => $this->resolveDocumentType($extension),
            'editorConfig' => [
                'mode' => $mode,
                'lang' => 'id',
                'callbackUrl' => $callbackUrl,
                'user' => [
                    'id' => (string) $user->id,
                    'name' => $user->name,
                ],
                'customization' => [
                    'forcesave' => $isEdit,
                    // compactToolbar only — do NOT set toolbarNoTabs.
                    // OnlyOffice api.js (9.4+) switches to /index_loader.html when
                    // toolbarNoTabs is true, and our Document Server package is
                    // missing main/*/index_loader.html (404) while index.html exists.
                    'compactToolbar' => ! $isEdit,
                    'feedback' => false,
                    'help' => false,
                    'compactHeader' => true,
                    'autosave' => $isEdit,
                ],
            ],
        ];

        $jwtSecret = (string) config('onlyoffice.jwt_secret');
        if ($jwtSecret !== '') {
            $config['token'] = OnlyOfficeJwt::encode($config, $jwtSecret);
        }

        return [
            'documentServerUrl' => rtrim((string) config('onlyoffice.document_server_url'), '/').'/',
            'config' => $config,
            'mode' => $mode,
            'can_edit' => $canEdit,
            'download_url' => $downloadUrl,
            'media' => [
                'id' => $media->id,
                'file_name' => $media->file_name,
                'mime_type' => $media->mime_type,
                'extension' => $extension,
            ],
        ];
    }

    public function parseDocumentKey(string $key): ?int
    {
        if (! preg_match('/^media_(\d+)_/', $key, $matches)) {
            return null;
        }

        return (int) $matches[1];
    }

    public function buildDocumentKey(Media $media): string
    {
        $version = $media->updated_at?->timestamp ?? time();

        return "media_{$media->id}_{$version}";
    }

    public function resolveDocumentType(string $extension): string
    {
        return match ($extension) {
            'xls', 'xlsx', 'ods', 'csv' => 'cell',
            'ppt', 'pptx', 'odp' => 'slide',
            default => 'word',
        };
    }

    /**
     * Overwrite media file in place so media_id stays stable.
     */
    public function overwriteMediaFile(Media $media, string $binaryContents): void
    {
        $path = $media->getPath();
        $directory = dirname($path);

        if (! is_dir($directory)) {
            mkdir($directory, 0755, true);
        }

        file_put_contents($path, $binaryContents);

        clearstatcache(true, $path);

        $media->size = (int) (filesize($path) ?: strlen($binaryContents));
        if (method_exists($media, 'touch')) {
            $media->touch();
        } else {
            $media->updated_at = now();
            $media->save();
        }

        // Clear generated conversions so previews refresh.
        if (method_exists($media, 'deleteGeneratedConversions')) {
            try {
                $media->deleteGeneratedConversions();
            } catch (\Throwable) {
                // Non-fatal for documents without conversions.
            }
        }
    }

    private function resolveMode(?string $requestedMode, bool $canEdit): string
    {
        $requested = strtolower((string) $requestedMode);

        if ($requested === 'edit') {
            abort_unless($canEdit, 403, 'Anda tidak memiliki izin mengedit dokumen ini.');

            return 'edit';
        }

        if ($requested === 'view') {
            return 'view';
        }

        // Default: edit when allowed, otherwise view.
        return $canEdit ? 'edit' : 'view';
    }

    /**
     * Callback URL MUST be reachable from the Document Server container.
     * The internal hostname differs from the public APP_URL, so allow override.
     */
    private function resolveCallbackUrl(): string
    {
        $override = trim((string) config('onlyoffice.callback_url'));
        if ($override !== '') {
            return rtrim($override, '/').'/api/onlyoffice/callback';
        }

        return url('/api/onlyoffice/callback');
    }
}
