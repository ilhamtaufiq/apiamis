<?php

namespace App\Http\Controllers;

use App\Http\Resources\SurveyTugasResource;
use App\Models\SurveyLokasi;
use App\Models\SurveyTugas;
use App\Models\User;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Validator;

class SurveyTugasController extends Controller
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

        $query = SurveyTugas::with(['pekerjaan', 'kecamatan', 'desa', 'assignee', 'assignees'])->withCount('surveys');

        if (!$user->hasRole('admin')) {
            $query->where(function ($q) use ($user) {
                $q->where('assignee_id', $user->id)
                    ->orWhereHas('assignees', fn ($a) => $a->where('users.id', $user->id));
            });
        }

        if ($request->filled('tahun_anggaran')) {
            $query->where('tahun_anggaran', $request->tahun_anggaran);
        }

        if ($request->filled('status')) {
            $query->where('status', $request->status);
        }

        if ($request->filled('assignee_id') && $user->hasRole('admin')) {
            $query->where(function ($q) use ($request) {
                $q->where('assignee_id', $request->assignee_id)
                    ->orWhereHas('assignees', fn ($a) => $a->where('users.id', $request->assignee_id));
            });
        }

        if ($request->filled('search')) {
            $search = '%' . $request->search . '%';
            $query->where(function ($q) use ($search) {
                $q->where('judul', 'LIKE', $search)
                    ->orWhere('lokasi_catatan', 'LIKE', $search);
            });
        }

        $perPage = (int) $request->get('per_page', 15);
        $perPage = max(1, min($perPage, 100));

        $tugas = $query->latest()->paginate($perPage);

        return SurveyTugasResource::collection($tugas);
    }

    /**
     * Aturan validasi store/update.
     */
    private function rules(bool $isUpdate = false): array
    {
        $required = $isUpdate ? 'sometimes|required' : 'required';

        return [
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'judul' => $required . '|string|max:255',
            'tahun_anggaran' => $required . '|integer|min:2020|max:2100',
            'jenis' => 'nullable|in:spam_perpipaan,spam_pengeboran,mck_individu,mck_komunal',
            'kecamatan_id' => 'nullable|exists:tbl_kecamatan,id',
            'desa_id' => 'nullable|exists:tbl_desa,id',
            'lokasi_catatan' => 'nullable|string',
            'assignee_id' => $required . '|exists:users,id',
            'assignee_ids' => 'nullable|array|min:1|max:20',
            'assignee_ids.*' => 'exists:users,id',
            'status' => 'nullable|in:ditugaskan,dikerjakan,selesai',
            'batas_waktu' => 'nullable|date',
            'catatan_admin' => 'nullable|string',
        ];
    }

    private const SURVEY_ROLE_NAMES = ['admin', 'tfl', 'pengawas', 'konsultan_pengawas', 'operator'];

    /**
     * @return array{ids: list<int>|null, error: \Illuminate\Http\JsonResponse|null}
     */
    private function resolveAssignees(Request $request): array
    {
        $ids = $request->input('assignee_ids');
        if ($ids === null && $request->filled('assignee_id')) {
            $ids = [$request->assignee_id];
        }
        if (!is_array($ids) || empty($ids)) {
            return ['ids' => null, 'error' => null];
        }

        $ids = array_values(array_unique(array_map('intval', $ids)));
        $users = User::whereIn('id', $ids)->get()->keyBy('id');
        foreach ($ids as $id) {
            $u = $users->get($id);
            if (!$u || !$u->hasRole(self::SURVEY_ROLE_NAMES)) {
                return [
                    'ids' => null,
                    'error' => response()->json([
                        'message' => 'Validation error',
                        'errors' => ['assignee_ids' => ['Penanggung jawab harus memiliki salah satu role: admin, tfl, pengawas, konsultan_pengawas, operator.']],
                    ], 422),
                ];
            }
        }

        return ['ids' => $ids, 'error' => null];
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

        $validator = Validator::make($request->all(), $this->rules());

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $resolved = $this->resolveAssignees($request);
        if ($resolved['error']) {
            return $resolved['error'];
        }
        if ($resolved['ids'] === null) {
            $assignee = User::find($request->assignee_id);
            if (!$assignee || !$assignee->hasRole(self::SURVEY_ROLE_NAMES)) {
                return response()->json([
                    'message' => 'Validation error',
                    'errors' => ['assignee_id' => ['Penanggung jawab harus memiliki salah satu role: admin, tfl, pengawas, konsultan_pengawas, operator.']],
                ], 422);
            }
        }

        $data = $request->only([
            'pekerjaan_id',
            'judul',
            'tahun_anggaran',
            'jenis',
            'kecamatan_id',
            'desa_id',
            'lokasi_catatan',
            'assignee_id',
            'batas_waktu',
            'catatan_admin',
        ]);
        $data['status'] = $request->get('status', 'ditugaskan');
        $data['created_by'] = $user->id;

        if (!empty($data['pekerjaan_id']) && empty($data['kecamatan_id']) && empty($data['desa_id'])) {
            $pekerjaan = \App\Models\Pekerjaan::find($data['pekerjaan_id']);
            if ($pekerjaan) {
                $data['kecamatan_id'] = $pekerjaan->kecamatan_id;
                $data['desa_id'] = $pekerjaan->desa_id;
            }
        }

        if ($resolved['ids'] !== null) {
            $data['assignee_id'] = $resolved['ids'][0];
        }

        $tugas = SurveyTugas::create($data);
        $tugas->syncAssignees($resolved['ids'] ?? [$tugas->assignee_id]);

        return (new SurveyTugasResource($tugas->load(['pekerjaan', 'kecamatan', 'desa', 'assignee', 'assignees', 'creator'])->loadCount('surveys')))
            ->response()
            ->setStatusCode(201);
    }

    /**
     * Display the specified resource.
     */
    public function show(SurveyTugas $surveyTugas)
    {
        $user = auth()->user();
        if (!$user) {
            return response()->json(['message' => 'Unauthorized'], 401);
        }

        $isAdmin = $user->hasRole('admin');
        if (!$isAdmin && !$surveyTugas->isAssignee($user->id)) {
            return response()->json(['message' => 'Forbidden'], 403);
        }

        $surveyTugas->load(['pekerjaan', 'kecamatan', 'desa', 'assignee', 'assignees', 'creator']);
        $surveyTugas->load(['surveys' => function ($q) {
            $q->with('user')->latest();
        }]);

        return new SurveyTugasResource($surveyTugas);
    }

    /**
     * Update the specified resource in storage.
     */
    public function update(Request $request, SurveyTugas $surveyTugas)
    {
        $validator = Validator::make($request->all(), $this->rules(true));

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        if ($request->filled('assignee_id') || $request->has('assignee_ids')) {
            $resolved = $this->resolveAssignees($request);
            if ($resolved['error']) {
                return $resolved['error'];
            }
            if ($resolved['ids'] !== null) {
                $request->merge(['assignee_id' => $resolved['ids'][0]]);
            }
        }

        $surveyTugas->update($request->only([
            'pekerjaan_id',
            'judul',
            'tahun_anggaran',
            'jenis',
            'kecamatan_id',
            'desa_id',
            'lokasi_catatan',
            'assignee_id',
            'status',
            'batas_waktu',
            'catatan_admin',
        ]));

        $assigneeIds = $request->input('assignee_ids');
        if (is_array($assigneeIds) && !empty($assigneeIds)) {
            $surveyTugas->syncAssignees($assigneeIds);
        } elseif ($request->filled('assignee_id')) {
            $surveyTugas->assignees()->syncWithoutDetaching([(int) $request->assignee_id]);
        }

        return new SurveyTugasResource($surveyTugas->load(['pekerjaan', 'kecamatan', 'desa', 'assignee', 'assignees', 'creator'])->loadCount('surveys'));
    }

    /**
     * Remove the specified resource from storage.
     */
    public function destroy(SurveyTugas $surveyTugas)
    {
        SurveyLokasi::where('tugas_id', $surveyTugas->id)->update(['tugas_id' => null]);
        $surveyTugas->delete();

        return response()->json(['message' => 'Tugas survey berhasil dihapus.']);
    }
}
