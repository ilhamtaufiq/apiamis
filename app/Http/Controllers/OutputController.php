<?php

namespace App\Http\Controllers;

use App\Models\Output;
use App\Http\Resources\OutputResource;
use Illuminate\Http\Request;

class OutputController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/output",
     *     summary="List all output",
     *     tags={"Output"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Output::with('pekerjaan');

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
            
            // If pekerjaan_id is provided, check if per_page is -1 to get all
            if ($request->get('per_page') == -1) {
                return OutputResource::collection($query->get());
            }
        }

        $per_page = $request->get('per_page', 20);
        
        if ($per_page == -1) {
            $output = $query->get();
        } else {
            $output = $query->paginate($per_page);
        }

        return OutputResource::collection($output);
    }

    /**
     * @OA\Post(
     *     path="/api/output",
     *     summary="Create new output",
     *     tags={"Output"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"pekerjaan_id", "komponen", "satuan", "volume"},
     *             @OA\Property(property="pekerjaan_id", type="integer"),
     *             @OA\Property(property="komponen", type="string"),
     *             @OA\Property(property="satuan", type="string"),
     *             @OA\Property(property="volume", type="number")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Output created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'komponen' => 'required|string|max:255',
            'satuan' => 'required|string|max:255',
            'volume' => 'required|numeric|min:0',
            'penerima_is_optional' => 'boolean',
        ]);

        $output = Output::create($validated);
        $output->load('pekerjaan');
        return new OutputResource($output);
    }

    /**
     * @OA\Get(
     *     path="/api/output/{id}",
     *     summary="Get output detail",
     *     tags={"Output"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Output $output)
    {
        $output->load('pekerjaan');
        return new OutputResource($output);
    }

    /**
     * @OA\Put(
     *     path="/api/output/{id}",
     *     summary="Update output",
     *     tags={"Output"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Output $output)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'komponen' => 'nullable|string|max:255',
            'satuan' => 'nullable|string|max:255',
            'volume' => 'nullable|numeric|min:0',
            'penerima_is_optional' => 'nullable|boolean',
        ]);

        $output->update($validated);
        $output->load('pekerjaan');
        return new OutputResource($output);
    }

    /**
     * @OA\Delete(
     *     path="/api/output/{id}",
     *     summary="Delete output",
     *     tags={"Output"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Output $output)
    {
        $output->delete();
        return response()->json(['message' => 'Output deleted successfully'], 200);
    }
    public function summary(Request $request)
    {
        $query = Output::query();

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        $total = (clone $query)->count();
        $wajib = (clone $query)->where('penerima_is_optional', false)->count();

        return response()->json([
            'total_output' => $total,
            'wajib_count' => $wajib,
            'opsional_count' => $total - $wajib,
        ]);
    }
}
