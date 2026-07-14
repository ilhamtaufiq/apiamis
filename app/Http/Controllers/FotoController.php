<?php

namespace App\Http\Controllers;

use App\Http\Resources\FotoResource;
use App\Models\Foto;
use App\Models\Pekerjaan;
use App\Services\KoordinatValidationService;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class FotoController extends Controller
{
    public function __construct(
        private readonly KoordinatValidationService $koordinatValidationService,
    ) {}
    /**
     * @OA\Get(
     *     path="/api/foto",
     *     summary="List all foto",
     *     tags={"Media Management (Foto)"},
     *
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="latest_only", in="query", required=false, @OA\Schema(type="boolean")),
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Foto::with(['pekerjaan', 'penerima', 'komponen'])
            // Cegah bocor foto lintas role: hanya foto dari pekerjaan yang diizinkan
            ->whereHas('pekerjaan', fn ($q) => $q->byUserRole());

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function ($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('search') && ! empty($request->search)) {
            $searchTerm = $request->search;
            $query->whereHas('pekerjaan', function ($q) use ($searchTerm) {
                $q->where('nama_paket', 'LIKE', '%'.$searchTerm.'%')
                    ->orWhere('kode_rekening', 'LIKE', '%'.$searchTerm.'%')
                    ->orWhereHas('kontrak.penyedia', function ($penyediaQuery) use ($searchTerm) {
                        $penyediaQuery->where('nama', 'LIKE', '%'.$searchTerm.'%');
                    });
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
            // Return all photos for a specific pekerjaan (no pagination)
            $foto = $query->get();

            return FotoResource::collection($foto);
        }

        if ($request->has('latest_only') && $request->latest_only) {
            $query->whereIn('id', function ($q) {
                $q->selectRaw('MAX(id)')
                    ->from('tbl_foto')
                    ->groupBy('pekerjaan_id');
            });
        }

        // Support for custom pagination or all results
        $per_page = $request->get('per_page', 20);

        if ($per_page == -1) {
            $foto = $query->get();
        } else {
            $foto = $query->paginate($per_page);
        }

        return FotoResource::collection($foto);
    }

    /**
     * @OA\Post(
     *     path="/api/foto",
     *     summary="Upload new foto",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *
     *     @OA\RequestBody(
     *         required=true,
     *
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *
     *             @OA\Schema(
     *                 required={"pekerjaan_id", "komponen_id", "keterangan", "koordinat", "file"},
     *
     *                 @OA\Property(property="pekerjaan_id", type="integer"),
     *                 @OA\Property(property="komponen_id", type="integer"),
     *                 @OA\Property(property="penerima_id", type="integer"),
     *                 @OA\Property(property="keterangan", type="string", enum={"0%","25%","50%","75%","100%"}),
     *                 @OA\Property(property="koordinat", type="string"),
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *
     *     @OA\Response(response=201, description="Foto uploaded")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'komponen_id' => 'required|integer',
            'penerima_id' => 'nullable|integer',
            'keterangan' => 'required|in:0%,25%,50%,75%,100%',
            'koordinat' => 'required|string|max:255',
            'unit_index' => 'nullable|integer',
            'file' => 'required|file|mimes:jpg,jpeg,png|max:51200', // Max 50MB and images only
        ]);

        $pekerjaan = Pekerjaan::query()->findOrFail($validated['pekerjaan_id']);
        if (! Pekerjaan::userCanAccess((int) $pekerjaan->id)) {
            abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
        }

        $koordinatValidation = $this->koordinatValidationService->validateForPekerjaan(
            $pekerjaan,
            $validated['koordinat'],
        );

        $validated['validasi_koordinat'] = $koordinatValidation['valid'];
        $validated['validasi_koordinat_message'] = $koordinatValidation['message'];

        $foto = Foto::create($validated);

        if ($request->hasFile('file')) {
            $foto->addMediaFromRequest('file')
                ->usingFileName(Str::uuid().'.'.$request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('foto/pekerjaan');
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);

        return new FotoResource($foto);
    }

    /**
     * @OA\Get(
     *     path="/api/foto/{id}",
     *     summary="Get foto detail",
     *     tags={"Media Management (Foto)"},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Foto $foto)
    {
        if (! Pekerjaan::userCanAccess((int) $foto->pekerjaan_id)) {
            abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);

        return new FotoResource($foto);
    }

    /**
     * @OA\Post(
     *     path="/api/foto/{id}",
     *     summary="Update foto",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Foto $foto)
    {
        if (! Pekerjaan::userCanAccess((int) $foto->pekerjaan_id)) {
            abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
        }

        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'komponen_id' => 'nullable|integer',
            'penerima_id' => 'nullable|integer',
            'keterangan' => 'nullable|in:0%,25%,50%,75%,100%',
            'koordinat' => 'nullable|string|max:255',
            'unit_index' => 'nullable|integer',
            'file' => 'nullable|file|mimes:jpg,jpeg,png|max:51200',
        ]);

        if (array_key_exists('koordinat', $validated) && $validated['koordinat'] !== null) {
            $pekerjaanId = $validated['pekerjaan_id'] ?? $foto->pekerjaan_id;
            $pekerjaan = Pekerjaan::query()->findOrFail($pekerjaanId);
            if (! Pekerjaan::userCanAccess((int) $pekerjaan->id)) {
                abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
            }
            $koordinatValidation = $this->koordinatValidationService->validateForPekerjaan(
                $pekerjaan,
                $validated['koordinat'],
            );

            $validated['validasi_koordinat'] = $koordinatValidation['valid'];
            $validated['validasi_koordinat_message'] = $koordinatValidation['message'];
        }

        // ConvertEmptyStringsToNull turns "" into null; never null-out non-nullable columns.
        foreach (['keterangan', 'koordinat', 'pekerjaan_id', 'komponen_id'] as $field) {
            if (array_key_exists($field, $validated) && $validated[$field] === null) {
                unset($validated[$field]);
            }
        }

        $foto->update($validated);

        // Hanya ganti media bila ada file valid. Jangan clearMedia dulu —
        // jika add gagal setelah clear, foto hilang permanen.
        $file = $request->file('file');
        if ($file && $file->isValid() && $file->getSize() > 0) {
            $newMedia = $foto->addMediaFromRequest('file')
                ->usingFileName(Str::uuid().'.'.$file->getClientOriginalExtension())
                ->toMediaCollection('foto/pekerjaan');

            foreach ($foto->getMedia('foto/pekerjaan') as $existing) {
                if ((int) $existing->id !== (int) $newMedia->id) {
                    $existing->delete();
                }
            }
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        $foto->loadMedia('foto/pekerjaan');

        return new FotoResource($foto);
    }

    /**
     * @OA\Delete(
     *     path="/api/foto/{id}",
     *     summary="Delete foto",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Foto $foto)
    {
        if (! Pekerjaan::userCanAccess((int) $foto->pekerjaan_id)) {
            abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
        }

        // Delete all media files from storage
        $foto->clearMediaCollection('foto/pekerjaan');

        // Delete the database record
        $foto->delete();

        return response()->json(['message' => 'Foto deleted successfully']);
    }
}
