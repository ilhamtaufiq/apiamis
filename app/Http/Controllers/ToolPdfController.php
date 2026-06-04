<?php

namespace App\Http\Controllers;

use App\Http\Resources\ToolPdfResource;
use App\Models\ToolPdf;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Str;
use Illuminate\Support\Facades\Validator;
use Illuminate\Validation\Rule;

class ToolPdfController extends Controller
{
    public function index(Request $request)
    {
        $query = ToolPdf::ownedBy($request->user()->id)
            ->with(['media', 'parent:id,name', 'signaturePlacements'])
            ->orderByDesc('created_at');

        if ($request->filled('kind') && $request->kind !== 'all') {
            $query->where('kind', $request->kind);
        }

        if ($request->filled('search')) {
            $search = $request->search;
            $query->where(function ($builder) use ($search) {
                $builder->where('name', 'like', "%{$search}%")
                    ->orWhere('original_filename', 'like', "%{$search}%");
            });
        }

        return ToolPdfResource::collection($query->get());
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'file' => 'required|file|mimes:pdf|max:51200',
            'name' => 'nullable|string|max:255',
            'placements' => 'nullable|string',
        ]);

        $pdf = DB::transaction(function () use ($request, $validated) {
            $file = $request->file('file');
            $pdf = ToolPdf::create([
                'user_id' => $request->user()->id,
                'name' => $validated['name'] ?: pathinfo($file->getClientOriginalName(), PATHINFO_FILENAME),
                'original_filename' => $file->getClientOriginalName(),
                'kind' => 'source',
            ]);

            $pdf->addMediaFromRequest('file')->toMediaCollection('pdf');

            $this->storeSignaturePlacements($pdf, $validated['placements'] ?? null);

            return $pdf->load(['media', 'signaturePlacements']);
        });

        return (new ToolPdfResource($pdf))
            ->response()
            ->setStatusCode(201);
    }

    public function sign(Request $request)
    {
        $validated = $request->validate([
            'file' => 'required|file|mimes:pdf|max:51200',
            'name' => 'nullable|string|max:255',
            'placements' => 'nullable|string',
            'source_id' => [
                'nullable',
                Rule::exists('tool_pdfs', 'id')
                    ->where(fn ($query) => $query
                        ->where('user_id', $request->user()->id)),
            ],
        ]);

        $signedPdf = DB::transaction(function () use ($request, $validated) {
            $file = $request->file('file');
            $source = null;

            if (!empty($validated['source_id'])) {
                $source = ToolPdf::ownedBy($request->user()->id)
                    ->findOrFail($validated['source_id']);
            }

            $signedPdf = ToolPdf::create([
                'user_id' => $request->user()->id,
                'parent_id' => $source?->id,
                'name' => $validated['name'] ?: pathinfo($file->getClientOriginalName(), PATHINFO_FILENAME),
                'original_filename' => $file->getClientOriginalName(),
                'kind' => 'signed',
            ]);

            $signedPdf->addMediaFromRequest('file')->toMediaCollection('pdf');

            $this->storeSignaturePlacements($signedPdf, $validated['placements'] ?? null);

            return $signedPdf->load(['media', 'parent:id,name', 'signaturePlacements']);
        });

        return (new ToolPdfResource($signedPdf))
            ->response()
            ->setStatusCode(201);
    }

    public function bulkDownload(Request $request)
    {
        $validated = $request->validate([
            'ids' => 'required|array|min:1',
            'ids.*' => [
                'required',
                Rule::exists('tool_pdfs', 'id')
                    ->where(fn ($query) => $query
                        ->where('user_id', $request->user()->id)),
            ],
        ]);

        $toolPdfs = ToolPdf::ownedBy($request->user()->id)
            ->whereIn('id', $validated['ids'])
            ->with(['media', 'signaturePlacements'])
            ->get()
            ->values();

        abort_if($toolPdfs->isEmpty(), 404, 'File PDF tidak ditemukan');

        $zipBase = tempnam(sys_get_temp_dir(), 'toolpdf_');
        abort_if($zipBase === false, 500, 'Gagal menyiapkan file zip');

        $zipPath = $zipBase . '.zip';
        if (file_exists($zipBase)) {
            @unlink($zipBase);
        }

        $zip = new \ZipArchive();
        $opened = $zip->open($zipPath, \ZipArchive::CREATE | \ZipArchive::OVERWRITE);
        abort_if($opened !== true, 500, 'Gagal membuat arsip ZIP');

        foreach ($toolPdfs as $index => $toolPdf) {
            $media = $toolPdf->getFirstMedia('pdf');
            if (! $media || ! file_exists($media->getPath())) {
                continue;
            }

            $name = $toolPdf->name ?: ($toolPdf->original_filename ?: 'document');
            $safeName = Str::slug(pathinfo($name, PATHINFO_FILENAME), '_') ?: 'document';
            $entryName = sprintf('%02d_%s.pdf', $index + 1, $safeName);

            $zip->addFile($media->getPath(), $entryName);
        }

        $zip->close();

        return response()
            ->download($zipPath, 'tool-pdfs-bulk.zip', ['Content-Type' => 'application/zip'])
            ->deleteFileAfterSend(true);
    }

    public function download(Request $request, ToolPdf $toolPdf): JsonResponse|\Symfony\Component\HttpFoundation\BinaryFileResponse
    {
        abort_unless($toolPdf->canManage($request->user()), 403, 'Anda tidak memiliki akses ke file PDF ini');

        $media = $toolPdf->getFirstMedia('pdf');
        abort_unless($media, 404, 'File PDF tidak ditemukan');

        return response()->file($media->getPath(), [
            'Content-Type' => $media->mime_type ?: 'application/pdf',
        ]);
    }

    public function destroy(Request $request, ToolPdf $toolPdf): JsonResponse
    {
        abort_unless($toolPdf->canManage($request->user()), 403, 'Anda tidak memiliki akses untuk menghapus file ini');

        $toolPdf->delete();

        return response()->json([
            'success' => true,
            'message' => 'File PDF berhasil dihapus',
        ]);
    }

    private function storeSignaturePlacements(ToolPdf $toolPdf, ?string $placementsJson): void
    {
        if ($placementsJson === null || trim($placementsJson) === '') {
            return;
        }

        $placements = json_decode($placementsJson, true);
        if (!is_array($placements)) {
            abort(422, 'Format placement tanda tangan tidak valid');
        }

        Validator::make(['placements' => $placements], [
            'placements' => 'array',
            'placements.*.signature_id' => 'required|string|max:64',
            'placements.*.page_number' => 'required|integer|min:1',
            'placements.*.x_ratio' => 'required|numeric|min:0|max:1',
            'placements.*.y_ratio' => 'required|numeric|min:0|max:1',
            'placements.*.scale' => 'required|numeric|min:0.01|max:1',
            'placements.*.sort_order' => 'nullable|integer|min:0',
            'placements.*.signature_name' => 'required|string|max:255',
            'placements.*.signature_file_name' => 'required|string|max:255',
            'placements.*.signature_mime_type' => 'required|string|max:100',
            'placements.*.signature_width' => 'required|integer|min:1|max:20000',
            'placements.*.signature_height' => 'required|integer|min:1|max:20000',
            'placements.*.signature_data_url' => [
                'nullable',
                'string',
                'regex:/^data:image\/(png|jpe?g|webp);base64,/i',
            ],
            'placements.*.signature_source_type' => 'nullable|in:upload,library',
            'placements.*.signature_source_id' => 'nullable|string|max:64',
        ])->validate();

        foreach ($placements as $index => $placement) {
            $toolPdf->signaturePlacements()->create([
                'signature_id' => (string) $placement['signature_id'],
                'page_number' => (int) $placement['page_number'],
                'x_ratio' => (float) $placement['x_ratio'],
                'y_ratio' => (float) $placement['y_ratio'],
                'scale' => (float) $placement['scale'],
                'sort_order' => isset($placement['sort_order']) ? (int) $placement['sort_order'] : $index,
                'signature_name' => (string) $placement['signature_name'],
                'signature_file_name' => (string) $placement['signature_file_name'],
                'signature_mime_type' => (string) $placement['signature_mime_type'],
                'signature_width' => (int) $placement['signature_width'],
                'signature_height' => (int) $placement['signature_height'],
                'signature_data_url' => $placement['signature_data_url'] ?? null,
                'signature_source_type' => $placement['signature_source_type'] ?? null,
                'signature_source_id' => $placement['signature_source_id'] ?? null,
            ]);
        }
    }
}
