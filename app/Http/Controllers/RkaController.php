<?php

namespace App\Http\Controllers;

use App\Http\Resources\RkaDocumentResource;
use App\Models\RkaDocument;
use App\Services\RkaPdfParser;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Storage;
use Illuminate\Validation\Rule;

class RkaController extends Controller
{
    public function index(Request $request)
    {
        $query = RkaDocument::query()
            ->withCount('items')
            ->latest();

        if ($request->filled('jenis')) {
            $query->where('jenis', $request->string('jenis'));
        }

        if ($request->filled('tahun')) {
            $query->where('tahun_anggaran', $request->string('tahun'));
        }

        return RkaDocumentResource::collection($query->paginate($request->integer('per_page', 15)));
    }

    public function show(RkaDocument $rkaDocument)
    {
        $rkaDocument->load('items');

        return new RkaDocumentResource($rkaDocument);
    }

    public function import(Request $request, RkaPdfParser $parser)
    {
        $validated = $request->validate([
            'jenis' => ['required', Rule::in(['murni', 'parsial'])],
            'file' => ['required', 'file', 'mimes:pdf', 'max:51200'],
        ]);

        $file = $request->file('file');
        $storedPath = $file->store('rka/pdf');
        $absolutePath = Storage::path($storedPath);

        $parsed = $parser->parse($absolutePath, $validated['jenis']);

        $document = DB::transaction(function () use ($parsed, $storedPath, $file, $request) {
            $meta = $parsed['meta'];
            $document = RkaDocument::create([
                'jenis' => $meta['jenis'],
                'nama_file' => $file->getClientOriginalName(),
                'path_file' => $storedPath,
                'path_text' => $parsed['text_path'],
                'nomor_dokumen' => $meta['nomor_dokumen'],
                'tahun_anggaran' => $meta['tahun_anggaran'],
                'program' => $meta['program'],
                'kegiatan' => $meta['kegiatan'],
                'sub_kegiatan' => $meta['sub_kegiatan'],
                'sumber_pendanaan' => $meta['sumber_pendanaan'],
                'total_sebelum' => $meta['total_sebelum'],
                'total_setelah' => $meta['total_setelah'],
                'total_selisih' => $meta['total_selisih'],
                'imported_by' => $request->user()?->id,
            ]);

            foreach ($parsed['items'] as $item) {
                $document->items()->create($item);
            }

            return $document;
        });

        return new RkaDocumentResource($document->loadCount('items'));
    }

    public function destroy(RkaDocument $rkaDocument)
    {
        if ($rkaDocument->path_file) {
            Storage::delete($rkaDocument->path_file);
        }

        $rkaDocument->delete();

        return response()->json(['message' => 'RKA berhasil dihapus']);
    }
}
