<?php

namespace App\Http\Controllers;

use App\Http\Resources\UsulanKegiatanResource;
use App\Models\UsulanKegiatan;
use App\Models\User;
use App\Notifications\AppNotification;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Notification;
use Illuminate\Support\Facades\Validator;
use Maatwebsite\Excel\Facades\Excel;
use App\Exports\UsulanKegiatanExport;

class UsulanKegiatanController extends Controller
{
    /**
     * Display a listing of the resource.
     */
    public function index(Request $request)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $query = UsulanKegiatan::with(['user', 'kecamatan', 'desa']);

        // Non-admin can only see their own usulan
        if (!$user->hasRole('admin')) {
            $query->where('user_id', $user->id);
        }

        // Filters
        if ($request->filled('search')) {
            $search = '%' . $request->search . '%';
            $query->where(function ($q) use ($search) {
                $q->where('nama_pengusul', 'LIKE', $search)
                  ->orWhere('perihal', 'LIKE', $search)
                  ->orWhere('ringkasan', 'LIKE', $search);
            });
        }

        if ($request->filled('sub_bidang')) {
            $query->where('sub_bidang', $request->sub_bidang);
        }

        if ($request->filled('kecamatan_id')) {
            $query->where('kecamatan_id', $request->kecamatan_id);
        }

        if ($request->filled('desa_id')) {
            $query->where('desa_id', $request->desa_id);
        }

        $perPage = $request->get('per_page', 20);
        $usulans = $query->latest()->paginate($perPage);

        return UsulanKegiatanResource::collection($usulans);
    }

    /**
     * Store a newly created resource in storage.
     */
    public function store(Request $request)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $validator = Validator::make($request->all(), [
            'sub_bidang' => 'required|in:air minum,sanitasi',
            'nama_pengusul' => 'required|string|max:255',
            'kecamatan_id' => 'required|exists:tbl_kecamatan,id',
            'desa_id' => 'required|exists:tbl_desa,id',
            'perihal' => 'required|string|max:255',
            'ringkasan' => 'required|string',
            'dokumen' => 'nullable|file|mimes:pdf,doc,docx,xls,xlsx,png,jpg,jpeg|max:10240', // max 10MB
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $usulan = UsulanKegiatan::create([
            'user_id' => $user->id,
            'sub_bidang' => $request->sub_bidang,
            'nama_pengusul' => $request->nama_pengusul,
            'kecamatan_id' => $request->kecamatan_id,
            'desa_id' => $request->desa_id,
            'perihal' => $request->perihal,
            'ringkasan' => $request->ringkasan,
        ]);

        if ($request->hasFile('dokumen')) {
            $usulan->addMediaFromRequest('dokumen')->toMediaCollection('dokumen');
        }

        // Notify Admins
        $admins = User::role('admin')->get();
        Notification::send($admins, new AppNotification(
            'Usulan Kegiatan Baru',
            'Usulan baru "' . $usulan->perihal . '" telah diajukan oleh ' . $usulan->nama_pengusul,
            '/usulan-kegiatan?id=' . $usulan->id,
            'info'
        ));

        return new UsulanKegiatanResource($usulan->load(['user', 'kecamatan', 'desa']));
    }

    /**
     * Display the specified resource.
     */
    public function show($id)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $usulan = UsulanKegiatan::with(['user', 'kecamatan', 'desa'])->findOrFail($id);

        if (!$user->hasRole('admin') && $usulan->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        return new UsulanKegiatanResource($usulan);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, $id)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $usulan = UsulanKegiatan::findOrFail($id);

        if (!$user->hasRole('admin') && $usulan->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        $validator = Validator::make($request->all(), [
            'sub_bidang' => 'sometimes|required|in:air minum,sanitasi',
            'nama_pengusul' => 'sometimes|required|string|max:255',
            'kecamatan_id' => 'sometimes|required|exists:tbl_kecamatan,id',
            'desa_id' => 'sometimes|required|exists:tbl_desa,id',
            'perihal' => 'sometimes|required|string|max:255',
            'ringkasan' => 'sometimes|required|string',
            'dokumen' => 'nullable|file|mimes:pdf,doc,docx,xls,xlsx,png,jpg,jpeg|max:10240',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $usulan->update($request->only([
            'sub_bidang',
            'nama_pengusul',
            'kecamatan_id',
            'desa_id',
            'perihal',
            'ringkasan',
        ]));

        if ($request->hasFile('dokumen')) {
            // Delete old media collection first if we want to replace it
            $usulan->clearMediaCollection('dokumen');
            $usulan->addMediaFromRequest('dokumen')->toMediaCollection('dokumen');
        }

        return new UsulanKegiatanResource($usulan->load(['user', 'kecamatan', 'desa']));
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy($id)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $usulan = UsulanKegiatan::findOrFail($id);

        if (!$user->hasRole('admin') && $usulan->user_id !== $user->id) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        $usulan->clearMediaCollection('dokumen');
        $usulan->delete();

        return response()->json(['message' => 'Usulan kegiatan berhasil dihapus.']);
    }

    public function exportExcel(Request $request)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        return Excel::download(new UsulanKegiatanExport(), 'rekap_usulan_kegiatan.xlsx');
    }
}
