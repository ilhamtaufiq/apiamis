<?php

namespace App\Http\Controllers;

use App\Http\Resources\UserDriveItemResource;
use App\Models\UserDriveItem;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Validation\Rule;

class UserDriveController extends Controller
{
    public function index(Request $request)
    {
        $validated = $request->validate([
            'parent_id' => 'nullable|integer',
            'search' => 'nullable|string|max:100',
            'page' => 'nullable|integer|min:1',
            'per_page' => 'nullable|integer|min:1|max:100',
        ]);

        $parentId = $request->input('parent_id');

        $query = UserDriveItem::ownedBy($request->user()->id)
            ->with('media')
            ->when(
                $parentId === null || $parentId === '' || $parentId === 'null',
                fn ($builder) => $builder->whereNull('parent_id'),
                fn ($builder) => $builder->where('parent_id', (int) $parentId),
            )
            ->orderByRaw("CASE WHEN kind = 'folder' THEN 0 ELSE 1 END")
            ->orderByDesc('updated_at');

        if (! empty($validated['search'] ?? null)) {
            $search = $validated['search'];
            $query->where(function ($builder) use ($search) {
                $builder->where('name', 'like', "%{$search}%")
                    ->orWhere('original_filename', 'like', "%{$search}%");
            });
        }

        $perPage = (int) ($validated['per_page'] ?? 48);

        return UserDriveItemResource::collection($query->paginate($perPage));
    }

    public function storeFolder(Request $request)
    {
        $validated = $request->validate([
            'name' => 'required|string|max:255',
            'parent_id' => [
                'nullable',
                'integer',
                Rule::exists('user_drive_items', 'id')
                    ->where(fn ($query) => $query
                        ->where('user_id', $request->user()->id)
                        ->where('kind', UserDriveItem::KIND_FOLDER)),
            ],
        ]);

        if (! empty($validated['parent_id'])) {
            $parent = UserDriveItem::ownedBy($request->user()->id)
                ->where('kind', UserDriveItem::KIND_FOLDER)
                ->findOrFail($validated['parent_id']);
            abort_unless($parent->canManage($request->user()), 403);
        }

        $folder = UserDriveItem::create([
            'user_id' => $request->user()->id,
            'parent_id' => $validated['parent_id'] ?? null,
            'name' => trim($validated['name']),
            'kind' => UserDriveItem::KIND_FOLDER,
        ]);

        return (new UserDriveItemResource($folder))
            ->response()
            ->setStatusCode(201);
    }

    public function storeFile(Request $request)
    {
        $validated = $request->validate([
            'file' => 'required|file|max:204800',
            'name' => 'nullable|string|max:255',
            'parent_id' => [
                'nullable',
                'integer',
                Rule::exists('user_drive_items', 'id')
                    ->where(fn ($query) => $query
                        ->where('user_id', $request->user()->id)
                        ->where('kind', UserDriveItem::KIND_FOLDER)),
            ],
        ]);

        if (! empty($validated['parent_id'])) {
            $parent = UserDriveItem::ownedBy($request->user()->id)
                ->where('kind', UserDriveItem::KIND_FOLDER)
                ->findOrFail($validated['parent_id']);
            abort_unless($parent->canManage($request->user()), 403);
        }

        $item = DB::transaction(function () use ($request, $validated) {
            $file = $request->file('file');
            $displayName = trim((string) ($validated['name'] ?? ''));
            if ($displayName === '') {
                $displayName = pathinfo($file->getClientOriginalName(), PATHINFO_FILENAME) ?: 'file';
            }

            $driveItem = UserDriveItem::create([
                'user_id' => $request->user()->id,
                'parent_id' => $validated['parent_id'] ?? null,
                'name' => $displayName,
                'original_filename' => $file->getClientOriginalName(),
                'kind' => UserDriveItem::KIND_FILE,
            ]);

            $driveItem->addMediaFromRequest('file')->toMediaCollection('drive-file');

            return $driveItem->load('media');
        });

        return (new UserDriveItemResource($item))
            ->response()
            ->setStatusCode(201);
    }

    public function show(Request $request, UserDriveItem $userDriveItem)
    {
        abort_unless($userDriveItem->canManage($request->user()), 403);

        return new UserDriveItemResource($userDriveItem->load('media'));
    }

    public function destroy(Request $request, UserDriveItem $userDriveItem): JsonResponse
    {
        abort_unless($userDriveItem->canManage($request->user()), 403);

        if ($userDriveItem->kind === UserDriveItem::KIND_FOLDER) {
            $hasChildren = UserDriveItem::ownedBy($request->user()->id)
                ->where('parent_id', $userDriveItem->id)
                ->exists();

            abort_if($hasChildren, 422, 'Folder tidak kosong. Hapus isi folder terlebih dahulu.');
        }

        $userDriveItem->delete();

        return response()->json([
            'success' => true,
            'message' => 'Item drive berhasil dihapus',
        ]);
    }
}