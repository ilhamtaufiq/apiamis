<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\KegiatanRole;
use App\Http\Resources\PekerjaanResource;
use App\Http\Resources\PekerjaanDetailResource;
use App\Http\Resources\FotoResource;
use App\Http\Resources\BerkasResource;
use App\Imports\PekerjaanImport;
use App\Exports\PekerjaanTemplateExport;
use Illuminate\Http\Request;
use Maatwebsite\Excel\Facades\Excel;

class PekerjaanController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/pekerjaan",
     *     summary="List all pekerjaan",
     *     description="Returns paginated list of pekerjaan based on user role and filters",
     *     tags={"Pekerjaan"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(
     *         name="tahun",
     *         in="query",
     *         description="Filter by year",
     *         required=false,
     *         @OA\Schema(type="string")
     *     ),
     *     @OA\Parameter(
     *         name="kecamatan_id",
     *         in="query",
     *         description="Filter by kecamatan ID",
     *         required=false,
     *         @OA\Schema(type="integer")
     *     ),
     *     @OA\Parameter(
     *         name="search",
     *         in="query",
     *         description="Search by name or code",
     *         required=false,
     *         @OA\Schema(type="string")
     *     ),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        if (!auth()->check()) {
                return response()->json(['message' => 'Unauthorized'], 401);
            }

            $query = Pekerjaan::with(['kecamatan', 'desa', 'kegiatan', 'tags', 'pengawas', 'pendamping'])
                ->withCount('penerima')
                ->byUserRole();  // Aman karena sudah check auth
            
            // Filter by tahun via kegiatan
            if ($request->has('tahun') && !empty($request->tahun)) {
                $query->whereHas('kegiatan', function($q) use ($request) {
                    $q->where('tahun_anggaran', $request->tahun);
                });
            }

            // Filter multi
            if ($request->has('kecamatan_id') && !empty($request->kecamatan_id)) {
                $query->where('kecamatan_id', $request->kecamatan_id);
            }

            if ($request->has('kegiatan_id') && !empty($request->kegiatan_id)) {
                $query->where('kegiatan_id', $request->kegiatan_id);
            }

            // Filter by tag
            if ($request->has('tag_id') && !empty($request->tag_id)) {
                $query->whereHas('tags', function($q) use ($request) {
                    $q->where('tbl_tags.id', $request->tag_id);
                });
            }

            // Filter by pengawas
            if ($request->has('pengawas_id') && !empty($request->pengawas_id)) {
                $query->where('pengawas_id', $request->pengawas_id);
            }
            
            // Search functionality
            if ($request->has('search') && !empty($request->search)) {
                $searchTerm = $request->search;
                $query->whereFullText(['nama_paket', 'kode_rekening'], $searchTerm);
            }
            
            // Sorting
            $sortBy = $request->get('sort_by', 'created_at');
            $sortDirection = $request->get('sort_direction', 'desc');
            
            $allowedSorts = ['id', 'nama_paket', 'kode_rekening', 'pagu', 'penerima_count', 'created_at'];
            
            if (in_array($sortBy, $allowedSorts)) {
                $query->orderBy($sortBy, $sortDirection);
            } else {
                $query->latest();
            }

            // Support fetching all records for dropdown (per_page=-1)
            if ($request->has('per_page') && $request->per_page == -1) {
                return PekerjaanResource::collection($query->get());
            }
            
            $perPage = $request->get('per_page', 20);
            $pekerjaan = $query->paginate($perPage);
                
            return PekerjaanResource::collection($pekerjaan);
    }

    /**
     * @OA\Post(
     *      path="/api/pekerjaan",
     *      operationId="storePekerjaan",
     *      tags={"Pekerjaan"},
     *      summary="Create new pekerjaan",
     *      description="Store new pekerjaan in database",
     *      @OA\RequestBody(
     *          required=true,
     *          description="Pekerjaan data",
     *          @OA\JsonContent(
     *              required={"nama_paket","kecamatan_id","desa_id","pagu"},
     *              @OA\Property(property="kode_rekening", type="string", example="1.2.03.01"),
     *              @OA\Property(property="nama_paket", type="string", example="Pembangunan Saluran Air di Desa Argapura"),
     *              @OA\Property(property="kecamatan_id", type="integer", example=1),
     *              @OA\Property(property="desa_id", type="integer", example=5),
     *              @OA\Property(property="kegiatan_id", type="integer", example=3),
     *              @OA\Property(property="pagu", type="number", format="float", example=250000000)
     *          )
     *      ),
     *      @OA\Response(
     *          response=201,
     *          description="Pekerjaan created successfully",
     *          @OA\JsonContent(
     *              @OA\Property(property="id", type="integer", example=1),
     *              @OA\Property(property="kode_rekening", type="string", example="1.2.03.01"),
     *              @OA\Property(property="nama_paket", type="string", example="Pembangunan Saluran Air"),
     *              @OA\Property(property="pagu", type="number", format="float", example=250000000),
     *              @OA\Property(property="kecamatan", type="object"),
     *              @OA\Property(property="desa", type="object"),
     *              @OA\Property(property="kegiatan", type="object")
     *          )
     *      ),
     *      @OA\Response(
     *          response=422,
     *          description="Validation error"
     *      )
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'kode_rekening' => 'nullable|string|max:225',
            'nama_paket' => 'required|string|max:225',
            'kecamatan_id' => 'required|integer|exists:tbl_kecamatan,id',
            'desa_id' => 'required|integer|exists:tbl_desa,id',
            'kegiatan_id' => 'nullable|integer|exists:tbl_kegiatan,id',
            'pagu' => 'required|numeric|min:0',
            'pengawas_id' => 'nullable|integer|exists:pengawas,id',
            'pendamping_id' => 'nullable|integer|exists:pengawas,id'
        ]);

        $pekerjaan = Pekerjaan::create($validated);
        
        // Sync tags if provided
        if ($request->has('tag_ids')) {
            $pekerjaan->tags()->sync($request->tag_ids);
        }
        
        $pekerjaan->load('kecamatan', 'desa', 'kegiatan', 'tags', 'pengawas', 'pendamping');
        return new PekerjaanDetailResource($pekerjaan);
    }
   /**
     * @OA\Get(
     *      path="/api/pekerjaan/{id}",
     *      operationId="getPekerjaanDetailByRole",
     *      tags={"Pekerjaan"},
     *      summary="Get detail pekerjaan (hanya jika diizinkan role)",

     *      @OA\Parameter(
     *          name="id",
     *          in="path",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="Detail pekerjaan jika diizinkan"
     *      )
     * )
     */
    public function show(Pekerjaan $pekerjaan)
    {
        // Check authentication first
        if (!auth()->check() || !auth()->user()) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $user = auth()->user();
        
        // Check apakah user boleh akses pekerjaan ini
        if (!$user->hasRole('admin')) {
            $hasAccess = Pekerjaan::where('tbl_pekerjaan.id', $pekerjaan->id)
                ->byUserRole()
                ->exists();

            if (!$hasAccess) {
                abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
            }
        }
        
        $pekerjaan->load([
            'kecamatan', 'desa', 'kegiatan', 
            'foto', 'berkas', 'output', 'penerima', 'kontrak.penyedia', 'tags', 'pengawas', 'pendamping',
            'progress'
        ]);
        
        return new PekerjaanDetailResource($pekerjaan);
    }

    /**
     * @OA\Put(
     *      path="/api/pekerjaan/{id}",
     *      operationId="updatePekerjaan",
     *      tags={"Pekerjaan"},
     *      summary="Update pekerjaan",
     *      description="Update existing pekerjaan",
     *      @OA\Parameter(
     *          name="id",
     *          in="path",
     *          description="Pekerjaan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\RequestBody(
     *          required=true,
     *          @OA\JsonContent(
     *              @OA\Property(property="kode_rekening", type="string", example="1.2.03.02"),
     *              @OA\Property(property="nama_paket", type="string", example="Pembangunan Saluran Air (Updated)"),
     *              @OA\Property(property="pagu", type="number", format="float", example=300000000)
     *          )
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="Pekerjaan updated successfully"
     *      ),
     *      @OA\Response(
     *          response=404,
     *          description="Pekerjaan not found"
     *      )
     * )
     */
    public function update(Request $request, Pekerjaan $pekerjaan)
    {
        $validated = $request->validate([
            'kode_rekening' => 'nullable|string|max:225',
            'nama_paket' => 'nullable|string|max:225',
            'kecamatan_id' => 'nullable|integer|exists:tbl_kecamatan,id',
            'desa_id' => 'nullable|integer|exists:tbl_desa,id',
            'kegiatan_id' => 'nullable|integer|exists:tbl_kegiatan,id',
            'pagu' => 'nullable|numeric|min:0',
            'pengawas_id' => 'nullable|integer|exists:pengawas,id',
            'pendamping_id' => 'nullable|integer|exists:pengawas,id'
        ]);

        $pekerjaan->update($validated);
        
        // Sync tags if provided
        if ($request->has('tag_ids')) {
            $pekerjaan->tags()->sync($request->tag_ids);
        }
        
        $pekerjaan->load('kecamatan', 'desa', 'kegiatan', 'tags', 'pengawas', 'pendamping');
        return new PekerjaanDetailResource($pekerjaan);
    }

    /**
     * @OA\Delete(
     *      path="/api/pekerjaan/{id}",
     *      operationId="deletePekerjaan",
     *      tags={"Pekerjaan"},
     *      summary="Delete pekerjaan",
     *      description="Delete existing pekerjaan",
     *      @OA\Parameter(
     *          name="id",
     *          in="path",
     *          description="Pekerjaan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="Pekerjaan deleted successfully"
     *      ),
     *      @OA\Response(
     *          response=404,
     *          description="Pekerjaan not found"
     *      )
     * )
     */
    public function destroy(Pekerjaan $pekerjaan)
    {
        $pekerjaan->delete();
        return response()->json(['message' => 'Pekerjaan deleted successfully'], 200);
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/kecamatan/{kecamatanId}",
     *      operationId="getPekerjaanByKecamatan",
     *      tags={"Pekerjaan - Filter"},
     *      summary="Get pekerjaan by kecamatan",
     *      description="Get all pekerjaan in specific kecamatan",
     *      @OA\Parameter(
     *          name="kecamatanId",
     *          in="path",
     *          description="Kecamatan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="List pekerjaan by kecamatan"
     *      )
     * )
     */
    public function byKecamatan(Request $request, $kecamatanId)
    {
        $query = Pekerjaan::where('kecamatan_id', $kecamatanId)
            ->with('kecamatan', 'desa', 'kegiatan', 'pengawas', 'pendamping');

        // Filter by tahun via kegiatan
        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('kegiatan', function ($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        $pekerjaan = $query->paginate(20);
        return PekerjaanResource::collection($pekerjaan);
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/desa/{desaId}",
     *      operationId="getPekerjaanByDesa",
     *      tags={"Pekerjaan - Filter"},
     *      summary="Get pekerjaan by desa",
     *      description="Get all pekerjaan in specific desa",
     *      @OA\Parameter(
     *          name="desaId",
     *          in="path",
     *          description="Desa ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="List pekerjaan by desa"
     *      )
     * )
     */
    public function byDesa($desaId)
    {
        $pekerjaan = Pekerjaan::where('desa_id', $desaId)
            ->with('kecamatan', 'desa', 'kegiatan', 'pengawas', 'pendamping')
            ->paginate(20);
        return PekerjaanResource::collection($pekerjaan);
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/kegiatan/{kegiatanId}",
     *      operationId="getPekerjaanByKegiatan",
     *      tags={"Pekerjaan - Filter"},
     *      summary="Get pekerjaan by kegiatan",
     *      description="Get all pekerjaan in specific kegiatan",
     *      @OA\Parameter(
     *          name="kegiatanId",
     *          in="path",
     *          description="Kegiatan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="List pekerjaan by kegiatan"
     *      )
     * )
     */
    public function byKegiatan($kegiatanId)
    {
        $pekerjaan = Pekerjaan::where('kegiatan_id', $kegiatanId)
            ->with('kecamatan', 'desa', 'kegiatan', 'pengawas', 'pendamping')
            ->paginate(20);
        return PekerjaanResource::collection($pekerjaan);
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/kecamatan/{kecamatanId}/desa/{desaId}",
     *      operationId="getPekerjaanByKecamatanDesa",
     *      tags={"Pekerjaan - Filter"},
     *      summary="Get pekerjaan by kecamatan and desa",
     *      description="Get all pekerjaan in specific kecamatan and desa",
     *      @OA\Parameter(
     *          name="kecamatanId",
     *          in="path",
     *          description="Kecamatan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Parameter(
     *          name="desaId",
     *          in="path",
     *          description="Desa ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="List pekerjaan by kecamatan and desa"
     *      )
     * )
     */
    public function byKecamatanDesa(Request $request, $kecamatanId, $desaId)
    {
        $query = Pekerjaan::where('kecamatan_id', $kecamatanId)
            ->where('desa_id', $desaId)
            ->with('kecamatan', 'desa', 'kegiatan', 'pengawas', 'pendamping');

        // Filter by tahun via kegiatan
        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('kegiatan', function ($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        $pekerjaan = $query->paginate(20);
        return PekerjaanResource::collection($pekerjaan);
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/stats/pagu-kecamatan/{kecamatanId}",
     *      operationId="getTotalPaguByKecamatan",
     *      tags={"Pekerjaan - Stats"},
     *      summary="Get total pagu by kecamatan",
     *      description="Calculate total pagu for specific kecamatan",
     *      @OA\Parameter(
     *          name="kecamatanId",
     *          in="path",
     *          description="Kecamatan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="Total pagu calculated",
     *          @OA\JsonContent(
     *              @OA\Property(property="kecamatan_id", type="integer", example=1),
     *              @OA\Property(property="total_pagu", type="number", format="float", example=1250000000)
     *          )
     *      )
     * )
     */
    public function totalPaguByKecamatan($kecamatanId)
    {
        $total = Pekerjaan::where('kecamatan_id', $kecamatanId)
            ->sum('pagu');
        
        return response()->json([
            'kecamatan_id' => $kecamatanId,
            'total_pagu' => $total
        ]);
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/stats/pagu-kegiatan/{kegiatanId}",
     *      operationId="getTotalPaguByKegiatan",
     *      tags={"Pekerjaan - Stats"},
     *      summary="Get total pagu by kegiatan",
     *      description="Calculate total pagu for specific kegiatan",
     *      @OA\Parameter(
     *          name="kegiatanId",
     *          in="path",
     *          description="Kegiatan ID",
     *          required=true,
     *          @OA\Schema(type="integer")
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="Total pagu calculated",
     *          @OA\JsonContent(
     *              @OA\Property(property="kegiatan_id", type="integer", example=3),
     *              @OA\Property(property="total_pagu", type="number", format="float", example=500000000)
     *          )
     *      )
     * )
     */
    public function totalPaguByKegiatan($kegiatanId)
    {
        $total = Pekerjaan::where('kegiatan_id', $kegiatanId)
            ->sum('pagu');
        
        return response()->json([
            'kegiatan_id' => $kegiatanId,
            'total_pagu' => $total
        ]);
    }
    public function media(Pekerjaan $pekerjaan)
    {
        $pekerjaan->load('foto', 'berkas');
        return response()->json([
            'foto' => FotoResource::collection($pekerjaan->foto),
            'berkas' => BerkasResource::collection($pekerjaan->berkas),
        ]);
    }

    /**
     * @OA\Post(
     *      path="/api/pekerjaan/import",
     *      operationId="importPekerjaan",
     *      tags={"Pekerjaan"},
     *      summary="Import pekerjaan from excel",

     *      @OA\RequestBody(
     *          required=true,
     *          @OA\MediaType(
     *              mediaType="multipart/form-data",
     *              @OA\Schema(
     *                  @OA\Property(
     *                      property="file",
     *                      type="string",
     *                      format="binary"
     *                  )
     *              )
     *          )
     *      ),
     *      @OA\Response(
     *          response=200,
     *          description="Import successful"
     *      )
     * )
     */
    public function import(Request $request)
    {
        $request->validate([
            'file' => 'required|mimes:xlsx,xls,csv'
        ]);

        try {
            $import = new PekerjaanImport;
            
            // Disable events during bulk import to prevent notification flood
            Pekerjaan::withoutEvents(function () use ($import, $request) {
                Excel::import($import, $request->file('file'));
            });
            
            $failures = $import->failures();
            
            if ($failures->count() > 0) {
                // ... existing error handling ...
                $errorMessages = [];
                foreach ($failures as $failure) {
                    $errorMessages[] = "Baris " . $failure->row() . ": " . implode(', ', $failure->errors());
                }
                
                return response()->json([
                    'message' => 'Import selesai dengan beberapa error',
                    'errors' => array_slice($errorMessages, 0, 10),
                    'error_count' => $failures->count()
                ], 422);
            }

            // Send a single summary notification to admins
            $user = auth()->user();
            $userName = $user ? $user->name : 'System';
            $admins = \App\Models\User::role('admin')->get();
            $notification = new \App\Notifications\AppNotification(
                "Import Pekerjaan Berhasil",
                "Sejumlah data pekerjaan telah berhasil diimport oleh $userName.",
                "/pekerjaan",
                'success'
            );
            
            foreach ($admins as $admin) {
                if ($user && $admin->id === $user->id) continue;
                $admin->notify($notification);
            }

            return response()->json(['message' => 'Data pekerjaan berhasil diimport'], 200);
        } catch (\Exception $e) {
            return response()->json(['message' => 'Gagal mengimport data: ' . $e->getMessage()], 500);
        }
    }

    /**
     * @OA\Get(
     *      path="/api/pekerjaan/import/template",
     *      operationId="downloadPekerjaanTemplate",
     *      tags={"Pekerjaan"},
     *      summary="Download excel template for import",

     *      @OA\Response(
     *          response=200,
     *          description="Template file download"
     *      )
     * )
     */
    public function downloadTemplate()
    {
        return Excel::download(new PekerjaanTemplateExport, 'template_import_pekerjaan.xlsx');
    }

    /**
     * Get register of all document numbers for monitoring
     */
    public function documentRegister(Request $request)
    {
        try {
            $query = Pekerjaan::has('kontrak')
                ->with(['kontrak.penyedia', 'kontrak.registers.type', 'kegiatan'])
                ->byUserRole();

            if ($request->has('tahun') && $request->tahun) {
                 $query->whereHas('kegiatan', function($q) use ($request) {
                     $q->where('tahun_anggaran', $request->tahun);
                 });
            }

            if ($request->has('search') && !empty($request->search)) {
                $searchTerm = $request->search;
                $query->where(function($q) use ($searchTerm) {
                    $q->where('nama_paket', 'LIKE', '%' . $searchTerm . '%');
                });
            }

            $perPage = (int) $request->get('per_page', 20);
            
            if ($perPage === -1) {
                $data = $query->get();
                return response()->json([
                    'data' => $data,
                    'meta' => [
                        'total' => $data->count(),
                    ]
                ]);
            }

            $data = $query->paginate($perPage);
            return response()->json($data);

        } catch (\Exception $e) {
            return response()->json([
                'error' => 'Server Error',
                'message' => $e->getMessage(),
                'file' => $e->getFile(),
                'line' => $e->getLine()
            ], 500);
        }
    }
}

