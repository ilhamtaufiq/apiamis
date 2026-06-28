<?php

namespace App\Services;

use App\Services\OnlyOffice\OnlyOfficeConverter;
use Spatie\MediaLibrary\MediaCollections\Models\Media;

class DocumentPdfConverter
{
    public function __construct(
        private readonly OnlyOfficeConverter $onlyOfficeConverter,
    ) {}

    public function convertMediaToPdf(Media $media): ?string
    {
        if (! file_exists($media->getPath())) {
            return null;
        }

        return $this->onlyOfficeConverter->convertMediaToPdf($media);
    }

    public function getSuggestedDownloadName(Media $media): string
    {
        return pathinfo($media->file_name, PATHINFO_FILENAME).'.pdf';
    }
}