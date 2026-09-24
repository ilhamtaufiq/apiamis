<?php

namespace App\Services;

use Illuminate\Http\Client\Response;
use Illuminate\Support\Facades\Http;

class PaperlessService
{
    protected string $baseUrl;
    protected string $token;

    public function __construct()
    {
        $this->baseUrl = rtrim((string) config('paperless.url', 'http://localhost:8000'), '/');
        $this->token = (string) config('paperless.token', '');
    }

    protected function client()
    {
        return Http::baseUrl($this->baseUrl)
            ->withHeaders([
                'Authorization' => 'Token ' . $this->token,
            ])
            ->acceptJson();
    }

    public function uploadDocument(string $filePath, string $filename, array $metadata = []): Response
    {
        $request = Http::baseUrl($this->baseUrl)
            ->withHeaders([
                'Authorization' => 'Token ' . $this->token,
            ])
            ->attach('document', file_get_contents($filePath), $filename);

        $payload = array_filter([
            'title' => $metadata['title'] ?? $filename,
            'created' => $metadata['created'] ?? null,
            'correspondent' => $metadata['correspondent_id'] ?? null,
            'document_type' => $metadata['document_type_id'] ?? null,
        ]);

        if (!empty($metadata['tags'])) {
            $payload['tags'] = (array) $metadata['tags'];
        }

        return $request->post('/api/documents/post_document/', $payload);
    }

    public function getDocument(int $id): Response
    {
        return $this->client()->get("/api/documents/{$id}/");
    }

    public function downloadDocument(int $id): Response
    {
        return $this->client()->get("/api/documents/{$id}/download/");
    }

    public function searchDocuments(string $query, int $page = 1): Response
    {
        return $this->client()->get('/api/documents/', [
            'query' => $query,
            'page' => $page,
        ]);
    }
}
