<?php

namespace App\Http\Controllers;

use App\Models\SpamKelembagaanShareLink;
use App\Models\SpamKelembagaanSubmission;
use App\Models\UnitSpam;
use App\Services\SpamKelembagaanShareService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class SpamKelembagaanShareController extends Controller
{
    public function __construct(
        private readonly SpamKelembagaanShareService $service
    ) {
    }

    // ── Public (tanpa login) ──────────────────────────────────────────

    public function publicShow(string $token): JsonResponse
    {
        $link = SpamKelembagaanShareLink::where('token', $token)->firstOrFail();

        if (! $link->isUsable()) {
            return response()->json([
                'success' => false,
                'message' => 'Link form tidak aktif, kedaluwarsa, atau kuota sudah penuh.',
                'data' => $this->service->publicFormData($link),
            ], 410);
        }

        return response()->json([
            'success' => true,
            'data' => $this->service->publicFormData($link),
        ]);
    }

    public function publicSubmit(Request $request, string $token): JsonResponse
    {
        $link = SpamKelembagaanShareLink::where('token', $token)->firstOrFail();

        $validated = $request->validate([
            'payload' => 'required|array',
            'submitter_name' => 'required|string|max:255',
            'submitter_phone' => 'nullable|string|max:50',
            'submitter_instansi' => 'nullable|string|max:255',
            'submitter_note' => 'nullable|string|max:2000',
            // Flat fields also accepted (merged into payload)
            'name' => 'nullable|string|max:255',
            'tahun_pembangunan' => 'nullable|string|max:100',
            'sumber_dana' => 'nullable|string|max:255',
            'program' => 'nullable|string|max:255',
            'sistem_layanan' => 'nullable|string|max:255',
            'sumber_mata_air_kap' => 'nullable|string|max:255',
            'sumber_air_tanah_kap' => 'nullable|string|max:255',
            'lain_lain_kap' => 'nullable|string|max:255',
            'tarif_dasar_hukum' => 'nullable|string|max:255',
            'iuran_nominal' => 'nullable|string|max:255',
            'pendapatan_bulan' => 'nullable|string|max:255',
            'biaya_operasional' => 'nullable|string|max:255',
            'pokmas' => 'nullable|string|max:255',
            'perdes' => 'nullable|string|max:255',
            'kepala' => 'nullable|string|max:255',
            'bendahara' => 'nullable|string|max:255',
            'sekretaris' => 'nullable|string|max:255',
        ]);

        $payload = array_merge(
            $validated['payload'] ?? [],
            collect($validated)->only(array_merge(
                SpamKelembagaanShareService::UNIT_FIELDS,
                SpamKelembagaanShareService::PENGELOLA_FIELDS,
            ))->all()
        );

        $submission = $this->service->createSubmission($link, $payload, [
            'submitter_name' => $validated['submitter_name'],
            'submitter_phone' => $validated['submitter_phone'] ?? null,
            'submitter_instansi' => $validated['submitter_instansi'] ?? null,
            'submitter_note' => $validated['submitter_note'] ?? null,
            'submitter_ip' => $request->ip(),
            'user_agent' => substr((string) $request->userAgent(), 0, 500),
        ]);

        return response()->json([
            'success' => true,
            'message' => 'Usulan berhasil dikirim dan menunggu verifikasi admin.',
            'data' => [
                'id' => $submission->id,
                'status' => $submission->status,
                'created_at' => $submission->created_at?->toIso8601String(),
            ],
        ], 201);
    }

    // ── Admin (auth) ──────────────────────────────────────────────────

    public function indexLinks(Request $request): JsonResponse
    {
        $query = SpamKelembagaanShareLink::with(['unitSpam.desa.kecamatan', 'creator'])
            ->orderByDesc('created_at');

        if ($request->filled('unit_spam_id')) {
            $query->where('unit_spam_id', $request->integer('unit_spam_id'));
        }
        if ($request->filled('is_active')) {
            $query->where('is_active', filter_var($request->is_active, FILTER_VALIDATE_BOOLEAN));
        }

        $links = $query->paginate($request->integer('per_page', 20));

        return response()->json([
            'success' => true,
            'data' => collect($links->items())->map(fn ($l) => $this->service->serializeLink($l)),
            'meta' => [
                'current_page' => $links->currentPage(),
                'last_page' => $links->lastPage(),
                'per_page' => $links->perPage(),
                'total' => $links->total(),
            ],
        ]);
    }

    public function storeLink(Request $request): JsonResponse
    {
        $validated = $request->validate([
            'unit_spam_id' => 'required|exists:tbl_unit_spam,id',
            'label' => 'nullable|string|max:255',
            'expires_at' => 'nullable|date|after:now',
            'max_submissions' => 'nullable|integer|min:1|max:1000',
            'admin_note' => 'nullable|string|max:2000',
        ]);

        UnitSpam::findOrFail($validated['unit_spam_id']);

        $link = SpamKelembagaanShareLink::create([
            'unit_spam_id' => $validated['unit_spam_id'],
            'created_by' => $request->user()?->id,
            'token' => SpamKelembagaanShareLink::generateToken(),
            'label' => $validated['label'] ?? null,
            'is_active' => true,
            'expires_at' => $validated['expires_at'] ?? null,
            'max_submissions' => $validated['max_submissions'] ?? null,
            'admin_note' => $validated['admin_note'] ?? null,
        ]);

        return response()->json([
            'success' => true,
            'message' => 'Link form berhasil dibuat.',
            'data' => $this->service->serializeLink($link->load(['unitSpam.desa.kecamatan', 'creator'])),
        ], 201);
    }

    public function updateLink(Request $request, SpamKelembagaanShareLink $shareLink): JsonResponse
    {
        $validated = $request->validate([
            'label' => 'nullable|string|max:255',
            'is_active' => 'nullable|boolean',
            'expires_at' => 'nullable|date',
            'max_submissions' => 'nullable|integer|min:1|max:1000',
            'admin_note' => 'nullable|string|max:2000',
        ]);

        $shareLink->fill(collect($validated)->except('is_active')->all());
        if ($request->has('is_active')) {
            $shareLink->is_active = $request->boolean('is_active');
        }
        $shareLink->save();

        return response()->json([
            'success' => true,
            'data' => $this->service->serializeLink($shareLink->fresh(['unitSpam.desa.kecamatan', 'creator'])),
        ]);
    }

    public function destroyLink(SpamKelembagaanShareLink $shareLink): JsonResponse
    {
        $shareLink->update(['is_active' => false]);

        return response()->json([
            'success' => true,
            'message' => 'Link form dinonaktifkan.',
        ]);
    }

    public function indexSubmissions(Request $request): JsonResponse
    {
        $query = SpamKelembagaanSubmission::with([
            'unitSpam.desa.kecamatan',
            'shareLink',
            'reviewer',
        ])->orderByDesc('created_at');

        if ($request->filled('status')) {
            $query->where('status', $request->string('status'));
        }
        if ($request->filled('unit_spam_id')) {
            $query->where('unit_spam_id', $request->integer('unit_spam_id'));
        }

        $page = $query->paginate($request->integer('per_page', 20));

        return response()->json([
            'success' => true,
            'data' => collect($page->items())->map(fn ($s) => $this->service->serializeSubmission($s)),
            'meta' => [
                'current_page' => $page->currentPage(),
                'last_page' => $page->lastPage(),
                'per_page' => $page->perPage(),
                'total' => $page->total(),
                'pending_count' => SpamKelembagaanSubmission::where('status', 'pending')->count(),
            ],
        ]);
    }

    public function showSubmission(SpamKelembagaanSubmission $submission): JsonResponse
    {
        return response()->json([
            'success' => true,
            'data' => $this->service->serializeSubmission($submission),
        ]);
    }

    public function approveSubmission(Request $request, SpamKelembagaanSubmission $submission): JsonResponse
    {
        $validated = $request->validate([
            'review_note' => 'nullable|string|max:2000',
        ]);

        $result = $this->service->approve(
            $submission,
            (int) $request->user()->id,
            $validated['review_note'] ?? null
        );

        return response()->json([
            'success' => true,
            'message' => 'Usulan disetujui dan data unit SPAM diperbarui.',
            'data' => $this->service->serializeSubmission($result),
        ]);
    }

    public function rejectSubmission(Request $request, SpamKelembagaanSubmission $submission): JsonResponse
    {
        $validated = $request->validate([
            'review_note' => 'nullable|string|max:2000',
        ]);

        $result = $this->service->reject(
            $submission,
            (int) $request->user()->id,
            $validated['review_note'] ?? null
        );

        return response()->json([
            'success' => true,
            'message' => 'Usulan ditolak.',
            'data' => $this->service->serializeSubmission($result),
        ]);
    }
}
