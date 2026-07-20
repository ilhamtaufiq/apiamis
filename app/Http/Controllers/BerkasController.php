<?php

namespace App\Http\Controllers;

use App\Models\AppSetting;
use App\Models\Berkas;
use App\Models\Pekerjaan;
use App\Models\PuspenMediaShare;
use App\Http\Resources\BerkasResource;
use App\Http\Resources\PuspenMediaShareResource;
use App\Services\DocumentPdfConverter;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Str;


class BerkasController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/berkas",
     *     summary="List all berkas",
     *     tags={"Media Management (Berkas)"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Berkas::with(['pekerjaan', 'uploader']);

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        // Filter file milik user login (panel pengawas: mine=1 / uploaded_by=me)
        // Role pengawas/konsultan_pengawas juga dapat berkas berjudul RAB/GAMBAR/NEGO
        // bila opsi pengaturan aktif.
        $user = $request->user();
        $uploadedBy = $request->query('uploaded_by');
        $isPrivileged = $user && $user->hasAnyRole(['admin', 'manager', 'super-admin', 'operator']);
        $isFieldPengawas = $user && $user->hasAnyRole(['pengawas', 'konsultan_pengawas']);
        $wantsOwnOnly = $request->boolean('mine') || $uploadedBy === 'me';
        // Role lapangan murni (bukan dual admin/operator): selalu batasi ke milik sendiri + shared.
        $forceFieldScope = $isFieldPengawas && ! $isPrivileged;

        if ($wantsOwnOnly || $forceFieldScope) {
            $userId = $user?->id;
            $sharedTitles = ($isFieldPengawas || $forceFieldScope)
                ? AppSetting::pengawasVisibleBerkasJuduls()
                : [];

            $query->where(function ($q) use ($userId, $sharedTitles) {
                $q->where('uploaded_by', $userId);

                if ($sharedTitles !== []) {
                    $q->orWhere(function ($shared) use ($sharedTitles) {
                        foreach ($sharedTitles as $index => $title) {
                            $method = $index === 0 ? 'whereRaw' : 'orWhereRaw';
                            $shared->{$method}('LOWER(TRIM(jenis_dokumen)) = ?', [mb_strtolower($title)]);
                        }
                    });
                }
            });
        } elseif ($uploadedBy !== null && $uploadedBy !== '') {
            $query->where('uploaded_by', (int) $uploadedBy);
        }

        $perPage = min(max((int) $request->query('per_page', 20), 1), 100);
        $berkas = $query->latest('id')->paginate($perPage);
        return BerkasResource::collection($berkas);
    }

    /**
     * @OA\Post(
     *     path="/api/berkas",
     *     summary="Upload new berkas",
     *     tags={"Media Management (Berkas)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"pekerjaan_id", "jenis_dokumen", "file"},
     *                 @OA\Property(property="pekerjaan_id", type="integer"),
     *                 @OA\Property(property="jenis_dokumen", type="string"),
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="Berkas uploaded")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'required|string|max:255',
            'file' => 'required|file|max:51200', // max 50MB
        ]);

        $berkas = Berkas::create([
            'pekerjaan_id' => $validated['pekerjaan_id'],
            'jenis_dokumen' => $validated['jenis_dokumen'],
            'uploaded_by' => $request->user()?->id,
        ]);

        // Upload file dengan Spatie MediaLibrary
        if ($request->hasFile('file')) {
            $originalName = $request->file('file')->getClientOriginalName();
            $berkas->addMediaFromRequest('file')
                ->usingName(pathinfo($originalName, PATHINFO_FILENAME) ?: $originalName)
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('berkas/dokumen');
        }

        $berkas->load(['pekerjaan', 'uploader']);
        return new BerkasResource($berkas);
    }

    /**
     * @OA\Get(
     *     path="/api/berkas/{id}",
     *     summary="Get berkas detail",
     *     tags={"Media Management (Berkas)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Berkas $berkas)
    {
        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    /**
     * @OA\Post(
     *     path="/api/berkas/{id}",
     *     summary="Update berkas",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Media Management (Berkas)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Berkas $berkas)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'nullable|string|max:255',
            'file' => 'nullable|file|max:51200', // max 50MB
        ]);

        $berkas->update($validated);

        if ($request->hasFile('file')) {
            $berkas->clearMediaCollection('berkas/dokumen');
            $berkas->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('berkas/dokumen');
        }

        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    /**
     * @OA\Delete(
     *     path="/api/berkas/{id}",
     *     summary="Delete berkas",
     *     tags={"Media Management (Berkas)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Berkas $berkas)
    {
        // Delete all media files from storage
        $berkas->clearMediaCollection('berkas/dokumen');
        
        // Delete the database record
        $berkas->delete();
        
        return response()->json(['message' => 'Berkas deleted successfully']);
    }
    public function convertToPdf(Berkas $berkas, DocumentPdfConverter $converter)
    {
        $media = $berkas->getFirstMedia('berkas/dokumen');
        if (! $media) {
            return response()->json(['message' => 'Berkas tidak ditemukan'], 404);
        }

        $outputPath = $converter->convertMediaToPdf($media);

        if (! $outputPath || ! file_exists($outputPath)) {
            $extension = strtolower(pathinfo($media->file_name, PATHINFO_EXTENSION));

            return response()->json([
                'message' => match ($extension) {
                    'pdf' => 'Berkas sudah berformat PDF.',
                    default => 'Gagal mengonversi berkas ke PDF melalui ONLYOFFICE. Pastikan Document Server aktif dan format file didukung.',
                },
            ], 500);
        }

        $downloadName = $converter->getSuggestedDownloadName($media);
        $deleteAfterSend = ! Str::endsWith(strtolower($media->file_name), '.pdf');

        return response()->download($outputPath, $downloadName, [
            'Content-Type' => 'application/pdf',
        ])->deleteFileAfterSend($deleteAfterSend);
    }

    public function quickShareForPekerjaan(Request $request, Pekerjaan $pekerjaan)
    {
        $validated = $request->validate([
            'berkas_ids' => 'nullable|array',
            'berkas_ids.*' => 'integer|exists:tbl_berkas,id',
            'title' => 'nullable|string|max:255',
            'description' => 'nullable|string|max:2000',
        ]);

        $berkasQuery = Berkas::query()
            ->where('pekerjaan_id', $pekerjaan->id);

        if (! empty($validated['berkas_ids'])) {
            $berkasQuery->whereIn('id', $validated['berkas_ids']);
        }

        $berkasItems = $berkasQuery->get();
        $attachedCount = 0;

        $share = DB::transaction(function () use ($request, $pekerjaan, $validated, $berkasItems, &$attachedCount) {
            $share = PuspenMediaShare::create([
                'user_id' => $request->user()->id,
                'title' => $validated['title'] ?? ('Berkas: '.$pekerjaan->nama_paket),
                'description' => $validated['description'] ?? 'Berbagi dokumen pekerjaan melalui Puspen Media Sharing.',
                'share_token' => $this->makeShareToken(),
                'is_public' => true,
            ]);

            foreach ($berkasItems as $berkas) {
                $media = $berkas->getFirstMedia('berkas/dokumen');

                if (! $media || ! file_exists($media->getPath())) {
                    continue;
                }

                $folderPath = $this->cleanFolderPath($berkas->jenis_dokumen);

                $share->addMedia($media->getPath())
                    ->preservingOriginal()
                    ->usingName($berkas->jenis_dokumen)
                    ->usingFileName($media->file_name)
                    ->withCustomProperties([
                        'source_media_id' => $media->id,
                        'source_model_type' => $media->model_type,
                        'source_model_id' => $media->model_id,
                        'folder_path' => $folderPath,
                        'berkas_id' => $berkas->id,
                    ])
                    ->toMediaCollection('shared-media');

                $attachedCount++;
            }

            return $share->load('media');
        });

        abort_unless($attachedCount > 0, 422, 'Tidak ada berkas yang dapat dibagikan.');

        return (new PuspenMediaShareResource($share))
            ->response()
            ->setStatusCode(201);
    }

    public function uploadFromUrl(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'jenis_dokumen' => 'required|string|max:255',
            'url' => 'required|url',
        ]);

        $berkas = Berkas::create([
            'pekerjaan_id' => $validated['pekerjaan_id'],
            'jenis_dokumen' => $validated['jenis_dokumen'],
        ]);

        try {
            $berkas->addMediaFromUrl($validated['url'])
                ->toMediaCollection('berkas/dokumen');
        } catch (\Exception $e) {
            $berkas->delete();
            return response()->json(['message' => 'Gagal mendownload file dari URL. Pastikan URL mengarah langsung ke file.'], 400);
        }

        $berkas->load('pekerjaan');
        return new BerkasResource($berkas);
    }

    private function makeShareToken(): string
    {
        do {
            $token = Str::random(32);
        } while (PuspenMediaShare::where('share_token', $token)->exists());

        return $token;
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
}
