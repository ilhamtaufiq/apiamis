<?php

namespace App\Services\OnlyOffice;

use App\Models\User;
use Illuminate\Support\Facades\URL;
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

    public function buildEditorPayload(User $user, Media $media): array
    {
        abort_unless($this->isEnabled(), 503, 'ONLYOFFICE Document Server belum dikonfigurasi.');
        abort_unless($this->supportsMedia($media), 422, 'Format file tidak didukung ONLYOFFICE.');
        abort_unless($this->authorizer->canAccess($user, $media), 403, 'Anda tidak memiliki akses ke dokumen ini.');

        $canEdit = $user->hasRole('admin');
        $extension = strtolower($media->extension ?: pathinfo($media->file_name, PATHINFO_EXTENSION));
        $documentKey = $this->buildDocumentKey($media);

        $downloadUrl = URL::temporarySignedRoute(
            'onlyoffice.media.download',
            now()->addMinutes((int) config('onlyoffice.download_token_ttl_minutes', 120)),
            ['media' => $media->id],
        );

        $config = [
            'document' => [
                'fileType' => $extension,
                'key' => $documentKey,
                'title' => $media->file_name,
                'url' => $downloadUrl,
                'permissions' => [
                    'edit' => $canEdit,
                    'download' => true,
                    'print' => true,
                    'review' => false,
                    'comment' => $canEdit,
                    'fillForms' => $canEdit,
                ],
            ],
            'documentType' => $this->resolveDocumentType($extension),
            'editorConfig' => [
                'mode' => $canEdit ? 'edit' : 'view',
                'lang' => 'id',
                'callbackUrl' => url('/api/onlyoffice/callback'),
                'user' => [
                    'id' => (string) $user->id,
                    'name' => $user->name,
                ],
                'customization' => [
                    'forcesave' => $canEdit,
                    'compactToolbar' => ! $canEdit,
                    'toolbarNoTabs' => ! $canEdit,
                ],
            ],
        ];

        $jwtSecret = (string) config('onlyoffice.jwt_secret');
        if ($jwtSecret !== '') {
            $config['token'] = OnlyOfficeJwt::encode($config, $jwtSecret);
        }

        return [
            'documentServerUrl' => config('onlyoffice.document_server_url'),
            'config' => $config,
            'mode' => $canEdit ? 'edit' : 'view',
            'media' => [
                'id' => $media->id,
                'file_name' => $media->file_name,
                'mime_type' => $media->mime_type,
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
}