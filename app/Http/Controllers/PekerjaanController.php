<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Models\KegiatanRole;
use App\Http\Resources\PekerjaanResource;
use App\Http\Resources\PekerjaanDetailResource;
use Illuminate\Http\Request;

class PekerjaanController extends Controller
{
    /**
     * @OA\Get(
     *      path="/api/pekerjaan",
     *      operationId="getPekerjaanByRole",
     *      tags={"Pekerjaan"},
     *      summary="Get pekerjaan berdasarkan role user",
     *      description="Otomatis filter pekerjaan berdasarkan kegiatan yang diizinkan untuk role user",
     *      security={{"bearerAuth":{}}},
     *      @OA\Response(
     *          response=200,
     *          description="Pekerjaan yang diizinkan untuk role user"
     *      )
     * )
     */
    public function index(Request $request)
    {
        if (!auth()->check()) {
                return response()->json(['message' => 'Unauthorized'], 401);
            }

            $query = Pekerjaan::with(['kecamatan', 'desa', 'kegiatan'])
                ->byUserRole();  // Aman karena sudah check auth
            
            // Filter by tahun via kegiatan
            if ($request->has('tahun') && $request->tahun) {
                $query->whereHas('kegiatan', function($q) use ($request) {
                    $q->where('tahun_anggaran', $request->tahun);
                });
            }
            
            // Search functionality
            if ($request->has('search') && !empty($request->search)) {
                $searchTerm = $request->search;
                $query->where(function($q) use ($searchTerm) {
                    $q->where('nama_paket', 'LIKE', '%' . $searchTerm . '%')
                      ->orWhere('kode_rekening', 'LIKE', '%' . $searchTerm . '%');
                });
            }
            
            $pekerjaan = $query->paginate(20);
                
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
            'pagu' => 'required|numeric|min:0'
        ]);

        $pekerjaan = Pekerjaan::create($validated);
        $pekerjaan->load('kecamatan', 'desa', 'kegiatan');
        return new PekerjaanDetailResource($pekerjaan);
    }
   /**
     * @OA\Get(
     *      path="/api/pekerjaan/{id}",
     *      operationId="getPekerjaanDetailByRole",
     *      tags={"Pekerjaan"},
     *      summary="Get detail pekerjaan (hanya jika diizinkan role)",
     *      security={{"bearerAuth":{}}},
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
            $userKegiatanIds = KegiatanRole::whereIn('role_id', $user->roles->pluck('id'))
                ->pluck('kegiatan_id')
                ->toArray();
                
            if (!in_array($pekerjaan->kegiatan_id, $userKegiatanIds)) {
                abort(403, 'Anda tidak memiliki akses untuk pekerjaan ini');
            }
        }
        
        $pekerjaan->load([
            'kecamatan', 'desa', 'kegiatan', 
            'foto', 'berkas', 'output', 'penerima', 'kontrak'
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
            'pagu' => 'nullable|numeric|min:0'
        ]);

        $pekerjaan->update($validated);
        $pekerjaan->load('kecamatan', 'desa', 'kegiatan');
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
    public function byKecamatan($kecamatanId)
    {
        $pekerjaan = Pekerjaan::where('kecamatan_id', $kecamatanId)
            ->with('kecamatan', 'desa', 'kegiatan')
            ->paginate(20);
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
            ->with('kecamatan', 'desa', 'kegiatan')
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
            ->with('kecamatan', 'desa', 'kegiatan')
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
    public function byKecamatanDesa($kecamatanId, $desaId)
    {
        $pekerjaan = Pekerjaan::where('kecamatan_id', $kecamatanId)
            ->where('desa_id', $desaId)
            ->with('kecamatan', 'desa', 'kegiatan')
            ->paginate(20);
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
}
