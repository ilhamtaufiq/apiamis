<?php

namespace App\Http\Controllers;

use App\Models\Tiket;
use App\Models\User;
use App\Http\Resources\TiketResource;
use App\Notifications\AppNotification;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Validator;

class TiketController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/tiket",
     *     summary="List all tickets",
     *     tags={"Ticketing"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="status", in="query", required=false, @OA\Schema(type="string", enum={"open","pending","closed"})),
     *     @OA\Parameter(name="kategori", in="query", required=false, @OA\Schema(type="string", enum={"bug","request","lapangan","document","other"})),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $user = auth()->user();
        $query = Tiket::with(['user', 'pekerjaan', 'comments.user']);

        // Jika bukan admin, hanya lihat tiket sendiri
        if (!$user->hasRole('admin')) {
            $query->where('user_id', $user->id);
        }

        // Filter by status
        if ($request->has('status') && $request->status) {
            $query->where('status', $request->status);
        }

        // Filter by kategori
        if ($request->has('kategori') && $request->kategori) {
            $query->where('kategori', $request->kategori);
        }

        // Filter by pekerjaan_id
        if ($request->has('pekerjaan_id') && $request->pekerjaan_id) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        $perPage = $request->get('per_page', 20);
        $tikets = $query->latest()->paginate($perPage);

        return TiketResource::collection($tikets);
    }

    /**
     * @OA\Post(
     *     path="/api/tiket",
     *     summary="Create new ticket",
     *     tags={"Ticketing"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"subjek", "deskripsi", "kategori", "prioritas"},
     *                 @OA\Property(property="subjek", type="string"),
     *                 @OA\Property(property="deskripsi", type="string"),
     *                 @OA\Property(property="kategori", type="string", enum={"bug","request","lapangan","document","other"}),
     *                 @OA\Property(property="prioritas", type="string", enum={"low","medium","high"}),
     *                 @OA\Property(property="pekerjaan_id", type="integer"),
     *                 @OA\Property(property="attachment", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="Ticket created")
     * )
     */
    public function store(Request $request)
    {
        $validator = Validator::make($request->all(), [
            'subjek' => 'required|string|max:255',
            'deskripsi' => 'required|string',
            'kategori' => 'required|in:bug,request,lapangan,document,other',
            'prioritas' => 'required|in:low,medium,high',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'attachment' => 'nullable|image|max:2048',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $tiket = Tiket::create([
            'user_id' => auth()->id(),
            'pekerjaan_id' => $request->pekerjaan_id,
            'subjek' => $request->subjek,
            'deskripsi' => $request->deskripsi,
            'kategori' => $request->kategori,
            'prioritas' => $request->prioritas,
            'status' => 'open',
        ]);

        if ($request->hasFile('attachment')) {
            $tiket->addMediaFromRequest('attachment')->toMediaCollection('attachment');
        }

        // Notify Admins
        $admins = User::role('admin')->get();
        Notification::send($admins, new AppNotification(
            'Tiket Baru: ' . $tiket->subjek,
            'Tiket baru telah dibuat oleh ' . auth()->user()->name,
            '/tiket?ticketId=' . $tiket->id,
            'info'
        ));

        return new TiketResource($tiket->load(['user', 'pekerjaan']));
    }

    /**
     * @OA\Get(
     *     path="/api/tiket/{id}",
     *     summary="Get ticket detail",
     *     tags={"Ticketing"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Tiket $tiket)
    {
        $user = auth()->user();
        
        // Authorization check
        if (!$user->hasRole('admin') && $tiket->user_id !== $user->id) {
            return response()->json(['message' => 'Unauthorized'], 403);
        }

        return new TiketResource($tiket->load(['user', 'pekerjaan', 'comments.user']));
    }

    /**
     * @OA\Post(
     *     path="/api/tiket/{id}",
     *     summary="Update ticket",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Ticketing"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Tiket $tiket)
    {
        $user = auth()->user();
        $isAdmin = $user->hasRole('admin');

        // Authorization check
        if (!$isAdmin && $tiket->user_id !== $user->id) {
            return response()->json(['message' => 'Unauthorized'], 403);
        }

        $validationRules = [];

        if ($isAdmin) {
            $validationRules = [
                'status' => 'sometimes|in:open,pending,closed',
                'admin_notes' => 'nullable|string',
            ];
        } else {
            // User can only edit if still open
            if ($tiket->status !== 'open') {
                return response()->json(['message' => 'Tiket yang sudah diproses tidak dapat diubah'], 403);
            }
            $validationRules = [
                'subjek' => 'sometimes|string|max:255',
                'deskripsi' => 'sometimes|string',
                'kategori' => 'sometimes|in:bug,request,lapangan,document,other',
                'prioritas' => 'sometimes|in:low,medium,high',
                'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
                'attachment' => 'nullable|image|max:2048',
            ];
        }

        $validator = Validator::make($request->all(), $validationRules);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        if ($isAdmin) {
            $oldStatus = $tiket->status;
            $tiket->update($request->only(['status', 'admin_notes']));
            
            // Notify user if status changed
            if ($oldStatus !== $tiket->status) {
                $tiket->user->notify(new AppNotification(
                    'Update Status Tiket',
                    'Tiket Anda "' . $tiket->subjek . '" telah diubah statusnya menjadi ' . strtoupper($tiket->status),
                    '/tiket?ticketId=' . $tiket->id,
                    'success'
                ));
            }
        } else {
            $tiket->update($request->except(['attachment', '_method']));
            
            if ($request->hasFile('attachment')) {
                $tiket->clearMediaCollection('attachment');
                $tiket->addMediaFromRequest('attachment')->toMediaCollection('attachment');
            }
        }

        return new TiketResource($tiket->load(['user', 'pekerjaan']));
    }

    /**
     * @OA\Delete(
     *     path="/api/tiket/{id}",
     *     summary="Delete ticket",
     *     tags={"Ticketing"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Tiket $tiket)
    {
        $user = auth()->user();

        // Admin or owner can delete, but owner only if still open
        if ($user->hasRole('admin')) {
            $tiket->delete();
            return response()->json(['message' => 'Tiket berhasil dihapus']);
        }

        if ($tiket->user_id === $user->id) {
            if ($tiket->status !== 'open') {
                 return response()->json(['message' => 'Tiket yang sudah diproses tidak dapat dihapus'], 403);
            }
            $tiket->delete();
            return response()->json(['message' => 'Tiket berhasil dihapus']);
        }

        return response()->json(['message' => 'Unauthorized'], 403);
    }
    /**
     * Bulk update tickets status
     */
    public function bulkUpdate(Request $request)
    {
        $user = auth()->user();
        if (!$user->hasRole('admin')) {
            return response()->json(['message' => 'Unauthorized'], 403);
        }

        $validator = Validator::make($request->all(), [
            'ids' => 'required|array',
            'ids.*' => 'exists:tbl_tiket,id',
            'status' => 'required|in:open,pending,closed',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $status = $request->status;
        $ids = $request->ids;

        Tiket::whereIn('id', $ids)->update(['status' => $status]);

        // Optional: Notify users (could be slow if many, maybe dispatch jobs)
        // For simplicity, we just return success for now
        
        return response()->json(['message' => count($ids) . ' tiket berhasil diperbarui']);
    }
}
