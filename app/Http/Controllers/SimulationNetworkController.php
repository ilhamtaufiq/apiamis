<?php

namespace App\Http\Controllers;

use App\Models\SimulationNetwork;
use App\Models\SimulationNetworkVersion;
use Illuminate\Http\Request;
use Illuminate\Http\JsonResponse;
use Illuminate\Support\Facades\DB;

class SimulationNetworkController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/simulation-networks",
     *     summary="List all simulation networks",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="owned_only", in="query", required=false, @OA\Schema(type="boolean")),
     *     @OA\Parameter(name="search", in="query", required=false, @OA\Schema(type="string")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request): JsonResponse
    {
        $user = $request->user();
        $perPage = $request->input('per_page', 15);

        $query = SimulationNetwork::accessibleBy($user->id)
            ->with(['user:id,name', 'pekerjaan:id,nama_paket']);

        // Filter by pekerjaan if specified
        if ($request->has('pekerjaan_id')) {
            $query->forPekerjaan($request->pekerjaan_id);
        }

        // Filter by ownership
        if ($request->boolean('owned_only')) {
            $query->ownedBy($user->id);
        }

        // Search by name
        if ($request->has('search')) {
            $query->where('name', 'like', '%' . $request->search . '%');
        }

        // Sort
        $sortBy = $request->input('sort_by', 'updated_at');
        $sortDir = $request->input('sort_dir', 'desc');
        $query->orderBy($sortBy, $sortDir);

        $networks = $query->paginate($perPage);

        // Add stats to each network
        $networks->getCollection()->transform(function ($network) {
            $network->stats = $network->stats;
            return $network;
        });

        return response()->json([
            'success' => true,
            'data' => $networks,
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/simulation-networks",
     *     summary="Create new simulation network",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"name", "network_data"},
     *             @OA\Property(property="name", type="string"),
     *             @OA\Property(property="description", type="string"),
     *             @OA\Property(property="pekerjaan_id", type="integer"),
     *             @OA\Property(property="network_data", type="object")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Created")
     * )
     */
    public function store(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'name' => 'required|string|max:255',
            'description' => 'nullable|string|max:1000',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'network_data' => 'required|array',
            'network_data.junctions' => 'present|array',
            'network_data.reservoirs' => 'present|array',
            'network_data.tanks' => 'present|array',
            'network_data.pipes' => 'present|array',
            'network_data.pumps' => 'present|array',
            'network_data.valves' => 'present|array',
            'simulation_settings' => 'nullable|array',
            'is_public' => 'nullable|boolean',
        ]);

        $network = SimulationNetwork::create([
            'name' => $validated['name'],
            'description' => $validated['description'] ?? null,
            'user_id' => $request->user()->id,
            'pekerjaan_id' => $validated['pekerjaan_id'] ?? null,
            'network_data' => $validated['network_data'],
            'simulation_settings' => $validated['simulation_settings'] ?? SimulationNetwork::defaultSettings(),
            'is_public' => $validated['is_public'] ?? false,
            'version' => 1,
        ]);

        $network->load(['user:id,name', 'pekerjaan:id,nama_paket']);

        return response()->json([
            'success' => true,
            'message' => 'Jaringan simulasi berhasil dibuat',
            'data' => $network,
        ], 201);
    }

    /**
     * @OA\Get(
     *     path="/api/simulation-networks/{id}",
     *     summary="Get network detail",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Request $request, int $id): JsonResponse
    {
        $network = SimulationNetwork::with(['user:id,name', 'pekerjaan:id,nama_paket,kecamatan_id,desa_id'])
            ->findOrFail($id);

        // Check access
        if (!$network->canView($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki akses ke jaringan ini',
            ], 403);
        }

        $network->stats = $network->stats;
        $network->can_edit = $network->canEdit($request->user());

        return response()->json([
            'success' => true,
            'data' => $network,
        ]);
    }

    /**
     * @OA\Put(
     *     path="/api/simulation-networks/{id}",
     *     summary="Update simulation network",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, int $id): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        // Check edit permission
        if (!$network->canEdit($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki izin untuk mengedit jaringan ini',
            ], 403);
        }

        $validated = $request->validate([
            'name' => 'sometimes|string|max:255',
            'description' => 'nullable|string|max:1000',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'network_data' => 'sometimes|array',
            'network_data.junctions' => 'present|array',
            'network_data.reservoirs' => 'present|array',
            'network_data.tanks' => 'present|array',
            'network_data.pipes' => 'present|array',
            'network_data.pumps' => 'present|array',
            'network_data.valves' => 'present|array',
            'simulation_settings' => 'nullable|array',
            'is_public' => 'nullable|boolean',
            'save_version' => 'nullable|boolean',
            'version_description' => 'nullable|string|max:255',
        ]);

        DB::transaction(function () use ($network, $validated, $request) {
            // Save version history if network_data is being updated
            if (isset($validated['network_data']) && $request->input('save_version', true)) {
                $network->saveVersion($validated['version_description'] ?? 'Update jaringan');
            }

            $network->update($validated);
        });

        $network->load(['user:id,name', 'pekerjaan:id,nama_paket']);
        $network->stats = $network->stats;

        return response()->json([
            'success' => true,
            'message' => 'Jaringan simulasi berhasil diperbarui',
            'data' => $network,
        ]);
    }

    /**
     * @OA\Delete(
     *     path="/api/simulation-networks/{id}",
     *     summary="Delete simulation network",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Request $request, int $id): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        // Check edit permission (only owner or admin can delete)
        if (!$network->canEdit($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki izin untuk menghapus jaringan ini',
            ], 403);
        }

        $network->delete();

        return response()->json([
            'success' => true,
            'message' => 'Jaringan simulasi berhasil dihapus',
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/simulation-networks/{id}/versions",
     *     summary="Get network version history",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function versions(Request $request, int $id): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        if (!$network->canView($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki akses ke jaringan ini',
            ], 403);
        }

        $versions = $network->versions()
            ->with('changedBy:id,name')
            ->select(['id', 'simulation_network_id', 'version', 'change_description', 'changed_by', 'created_at'])
            ->get();

        return response()->json([
            'success' => true,
            'data' => $versions,
        ]);
    }

    /**
     * @OA\Get(
     *     path="/api/simulation-networks/{id}/versions/{version}",
     *     summary="Get network version detail",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="version", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function showVersion(Request $request, int $id, int $version): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        if (!$network->canView($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki akses ke jaringan ini',
            ], 403);
        }

        $versionData = $network->versions()
            ->where('version', $version)
            ->with('changedBy:id,name')
            ->firstOrFail();

        return response()->json([
            'success' => true,
            'data' => $versionData,
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/simulation-networks/{id}/restore/{version}",
     *     summary="Restore to specific version",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="version", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Restored")
     * )
     */
    public function restoreVersion(Request $request, int $id, int $version): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        if (!$network->canEdit($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki izin untuk mengedit jaringan ini',
            ], 403);
        }

        $success = $network->restoreToVersion($version);

        if (!$success) {
            return response()->json([
                'success' => false,
                'message' => 'Versi tidak ditemukan',
            ], 404);
        }

        $network->load(['user:id,name', 'pekerjaan:id,nama_paket']);
        $network->stats = $network->stats;

        return response()->json([
            'success' => true,
            'message' => "Berhasil restore ke versi {$version}",
            'data' => $network,
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/simulation-networks/{id}/results",
     *     summary="Save simulation results",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             @OA\Property(property="results", type="object")
     *         )
     *     ),
     *     @OA\Response(response=200, description="Results saved")
     * )
     */
    public function saveResults(Request $request, int $id): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        if (!$network->canEdit($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki izin untuk mengedit jaringan ini',
            ], 403);
        }

        $validated = $request->validate([
            'results' => 'required|array',
        ]);

        $network->update([
            'last_results' => $validated['results'],
            'last_simulated_at' => now(),
        ]);

        return response()->json([
            'success' => true,
            'message' => 'Hasil simulasi berhasil disimpan',
        ]);
    }

    /**
     * @OA\Post(
     *     path="/api/simulation-networks/{id}/duplicate",
     *     summary="Duplicate a network",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\RequestBody(
     *         @OA\JsonContent(
     *             @OA\Property(property="name", type="string")
     *         )
     *     ),
     *     @OA\Response(response=201, description="Duplicated")
     * )
     */
    public function duplicate(Request $request, int $id): JsonResponse
    {
        $network = SimulationNetwork::findOrFail($id);

        if (!$network->canView($request->user())) {
            return response()->json([
                'success' => false,
                'message' => 'Anda tidak memiliki akses ke jaringan ini',
            ], 403);
        }

        $newName = $request->input('name', $network->name . ' (Copy)');

        $newNetwork = SimulationNetwork::create([
            'name' => $newName,
            'description' => $network->description,
            'user_id' => $request->user()->id,
            'pekerjaan_id' => $network->pekerjaan_id,
            'network_data' => $network->network_data,
            'simulation_settings' => $network->simulation_settings,
            'is_public' => false,
            'version' => 1,
        ]);

        $newNetwork->load(['user:id,name', 'pekerjaan:id,nama_paket']);
        $newNetwork->stats = $newNetwork->stats;

        return response()->json([
            'success' => true,
            'message' => 'Jaringan berhasil diduplikasi',
            'data' => $newNetwork,
        ], 201);
    }

    /**
     * @OA\Get(
     *     path="/api/pekerjaan/{pekerjaanId}/simulation-networks",
     *     summary="Get networks linked to pekerjaan",
     *     tags={"Simulation Network"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="pekerjaanId", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function byPekerjaan(Request $request, int $pekerjaanId): JsonResponse
    {
        $user = $request->user();

        $networks = SimulationNetwork::accessibleBy($user->id)
            ->forPekerjaan($pekerjaanId)
            ->with(['user:id,name'])
            ->get();

        $networks->transform(function ($network) use ($user) {
            $network->stats = $network->stats;
            $network->can_edit = $network->canEdit($user);
            return $network;
        });

        return response()->json([
            'success' => true,
            'data' => $networks,
        ]);
    }
}
