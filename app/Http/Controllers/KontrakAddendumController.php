<?php

namespace App\Http\Controllers;

use App\Http\Resources\KontrakAddendumResource;
use App\Models\Kontrak;
use App\Models\KontrakAddendum;
use App\Models\Pekerjaan;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Validation\Rule;

class KontrakAddendumController extends Controller
{
    private const REQUIRED_ATTACHMENT_TYPES = [
        'surat_permohonan' => 'Surat Permohonan',
        'surat_undangan_pembahasan' => 'Surat Undangan Pembahasan',
        'risalah_rapat_pembahasan' => 'Risalah Rapat Pembahasan',
        'surat_perintah_pelaksanaan_kerja_sesuai_addendum' => 'Surat Perintah Pelaksanaan Kerja Sesuai Addendum',
        'cco' => 'CCO',
        'laporan_pekerjaan' => 'Laporan Pekerjaan',
        'berita_acara' => 'Berita Acara',
        'sk_peneliti_kontrak' => 'SK Peneliti Kontrak',
    ];

    public function registerGaps(\App\Services\KontrakAddendumRegisterGapService $gapService)
    {
        $this->authorizeAdmin();

        return response()->json($gapService->findGaps());
    }

    public function notifyRegisterGapPengawas(
        int $registerId,
        \App\Services\KontrakAddendumPengawasInstructionService $instructionService,
    ) {
        $this->authorizeAdmin();

        return response()->json($instructionService->notifyByRegisterId($registerId));
    }

    public function registerGapsForKontrak(
        Kontrak $kontrak,
        \App\Services\KontrakAddendumRegisterGapService $gapService,
    ) {
        $this->authorizeViewKontrak($kontrak);

        return response()->json($gapService->findGapsForKontrak($kontrak));
    }

    public function all(Request $request)
    {
        $this->authorizeAdmin();

        $query = KontrakAddendum::with([
            'kontrak.pekerjaan',
            'kontrak.pekerjaans',
            'kontrak.penyedia',
            'items',
            'media',
            'creator',
            'approver',
        ])
            ->orderByDesc('tanggal_addendum')
            ->orderByDesc('created_at');

        if ($request->filled('status') && $request->status !== 'all') {
            $query->where('status', $request->status);
        }

        if ($request->filled('search')) {
            $search = $request->search;
            $query->where(function ($q) use ($search) {
                $q->where('nomor_addendum', 'like', "%{$search}%")
                    ->orWhere('alasan', 'like', "%{$search}%")
                    ->orWhereHas('kontrak.pekerjaan', function ($pekerjaan) use ($search) {
                        $pekerjaan->where('nama_paket', 'like', "%{$search}%");
                    })
                    ->orWhereHas('kontrak.pekerjaans', function ($pekerjaan) use ($search) {
                        $pekerjaan->where('nama_paket', 'like', "%{$search}%");
                    })
                    ->orWhereHas('kontrak.penyedia', function ($penyedia) use ($search) {
                        $penyedia->where('nama', 'like', "%{$search}%");
                    });
            });
        }

        return KontrakAddendumResource::collection($query->paginate(20));
    }

    public function index(Kontrak $kontrak)
    {
        $this->authorizeViewKontrak($kontrak);

        $addendums = $kontrak->addendums()
            ->with(['items', 'creator', 'approver', 'media'])
            ->get();

        return KontrakAddendumResource::collection($addendums);
    }

    public function store(Request $request, Kontrak $kontrak)
    {
        $this->authorizeCreate($kontrak);

        $validated = $this->validateAddendum($request, $kontrak);
        $this->validateRequiredAttachmentsForPengawas($request);

        $addendum = DB::transaction(function () use ($validated, $kontrak) {
            $items = $validated['items'] ?? [];
            unset($validated['items'], $validated['attachments']);

            $addendum = $kontrak->addendums()->create([
                ...$validated,
                'status' => 'draft',
                'created_by' => auth()->id(),
            ]);

            $this->syncItems($addendum, $items);
            $this->storeTypedAttachments($addendum);

            return $addendum;
        });

        return (new KontrakAddendumResource($addendum->load(['items', 'media'])))
            ->response()
            ->setStatusCode(201);
    }

    public function show(KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeViewKontrak($kontrakAddendum->kontrak);

        return new KontrakAddendumResource(
            $kontrakAddendum->load([
                'kontrak.pekerjaan',
                'kontrak.pekerjaans',
                'kontrak.penyedia',
                'items',
                'creator',
                'approver',
                'media',
            ])
        );
    }

    public function update(Request $request, KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeAdmin();
        $this->ensureEditable($kontrakAddendum);

        $validated = $this->validateAddendum($request, $kontrakAddendum->kontrak, $kontrakAddendum);

        DB::transaction(function () use ($validated, $kontrakAddendum) {
            $items = $validated['items'] ?? null;
            unset($validated['items']);

            $kontrakAddendum->update($validated);

            if (is_array($items)) {
                $kontrakAddendum->items()->delete();
                $this->syncItems($kontrakAddendum, $items);
            }
        });

        return new KontrakAddendumResource($kontrakAddendum->fresh()->load(['items', 'media']));
    }

    public function destroy(KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeAdmin();
        $this->ensureEditable($kontrakAddendum);

        $kontrakAddendum->delete();

        return response()->json(['message' => 'Addendum kontrak berhasil dihapus']);
    }

    public function submit(KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeSubmit($kontrakAddendum);

        if (! in_array($kontrakAddendum->status, ['draft', 'ditolak'], true)) {
            return response()->json([
                'message' => 'Addendum hanya bisa diajukan dari status draft atau ditolak',
            ], 422);
        }

        $this->ensureRequiredAttachmentsExist($kontrakAddendum);

        $kontrakAddendum->update([
            'status' => 'diajukan',
            'approved_by' => null,
            'approved_at' => null,
        ]);

        return new KontrakAddendumResource($kontrakAddendum->fresh()->load(['items', 'media']));
    }

    public function approve(KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeAdmin();

        if (! in_array($kontrakAddendum->status, ['diajukan', 'draft'], true)) {
            return response()->json([
                'message' => 'Hanya addendum yang sudah diajukan atau draft yang bisa disetujui',
            ], 422);
        }

        $validated = request()->validate([
            'nomor_addendum' => 'required|string|max:100',
        ]);

        $kontrakAddendum->update([
            'nomor_addendum' => $validated['nomor_addendum'],
            'status' => 'disetujui',
            'approved_by' => auth()->id(),
            'approved_at' => now(),
        ]);

        return new KontrakAddendumResource($kontrakAddendum->fresh()->load(['items', 'media']));
    }

    public function overrideKelengkapan(KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeAdmin();

        if ($kontrakAddendum->status === 'disetujui') {
            return response()->json([
                'message' => 'Addendum yang sudah disetujui tidak bisa diubah kelengkapannya',
            ], 422);
        }

        $validated = request()->validate([
            'kelengkapan_override' => 'required|boolean',
        ]);

        $kontrakAddendum->update([
            'kelengkapan_override' => (bool) $validated['kelengkapan_override'],
        ]);

        return new KontrakAddendumResource($kontrakAddendum->fresh()->load(['items', 'media']));
    }

    public function reject(KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeAdmin();

        if ($kontrakAddendum->status !== 'diajukan') {
            return response()->json([
                'message' => 'Hanya addendum yang sudah diajukan yang bisa ditolak',
            ], 422);
        }

        $kontrakAddendum->update([
            'status' => 'ditolak',
            'approved_by' => auth()->id(),
            'approved_at' => now(),
        ]);

        return new KontrakAddendumResource($kontrakAddendum->fresh()->load(['items', 'media']));
    }

    public function upload(Request $request, KontrakAddendum $kontrakAddendum)
    {
        $this->authorizeAdmin();
        $this->ensureEditable($kontrakAddendum);

        $request->validate([
            'file' => 'required|file|mimes:pdf,doc,docx,xls,xlsx,jpg,jpeg,png|max:10240',
        ]);

        $kontrakAddendum
            ->addMediaFromRequest('file')
            ->toMediaCollection('kontrak/addendum');

        return new KontrakAddendumResource($kontrakAddendum->fresh()->load(['items', 'media']));
    }

    private function validateAddendum(Request $request, Kontrak $kontrak, ?KontrakAddendum $current = null): array
    {
        return $request->validate([
            'addendum_ke' => [
                'required',
                'integer',
                'min:1',
                Rule::unique('tbl_kontrak_addendums', 'addendum_ke')
                    ->where('kontrak_id', $kontrak->id)
                    ->ignore($current?->id),
            ],
            'nomor_addendum' => 'nullable|string|max:100',
            'tanggal_addendum' => 'required|date',
            'jenis_addendum' => 'required|in:teknis,biaya,waktu,teknis_biaya,lainnya',
            'alasan' => 'nullable|string',
            'deskripsi_perubahan' => 'nullable|string',
            'nilai_kontrak_sebelum' => 'nullable|numeric|min:0',
            'nilai_kontrak_sesudah' => 'nullable|numeric|min:0',
            'tgl_selesai_sebelum' => 'nullable|date',
            'tgl_selesai_sesudah' => 'nullable|date',
            'items' => 'nullable|array',
            'items.*.nama_item' => 'nullable|string|max:255',
            'items.*.spesifikasi_sebelum' => 'nullable|string',
            'items.*.spesifikasi_sesudah' => 'nullable|string',
            'items.*.volume_sebelum' => 'nullable|numeric',
            'items.*.volume_sesudah' => 'nullable|numeric',
            'items.*.harga_sebelum' => 'nullable|numeric|min:0',
            'items.*.harga_sesudah' => 'nullable|numeric|min:0',
            'items.*.subtotal_sebelum' => 'nullable|numeric|min:0',
            'items.*.subtotal_sesudah' => 'nullable|numeric|min:0',
            'attachments' => 'nullable|array',
            'attachments.*' => 'nullable|file|mimes:pdf,doc,docx,jpg,jpeg,png|max:10240',
        ]);
    }

    private function syncItems(KontrakAddendum $addendum, array $items): void
    {
        foreach ($items as $item) {
            $addendum->items()->create($item);
        }
    }

    private function validateRequiredAttachmentsForPengawas(Request $request): void
    {
        if (auth()->user()?->hasRole('admin')) {
            return;
        }

        $rules = [];
        $attributes = [];

        foreach (self::REQUIRED_ATTACHMENT_TYPES as $type => $label) {
            $rules["attachments.{$type}"] = $type === 'cco'
                ? 'required|file|mimes:pdf,xls,xlsx|max:10240'
                : 'required|file|mimes:pdf|max:10240';
            $attributes["attachments.{$type}"] = $label;
        }

        $request->validate($rules, [], $attributes);
    }

    private function storeTypedAttachments(KontrakAddendum $addendum): void
    {
        $attachments = request()->file('attachments', []);

        foreach ($attachments as $type => $file) {
            if (! $file || ! array_key_exists($type, self::REQUIRED_ATTACHMENT_TYPES)) {
                continue;
            }

            $addendum
                ->addMedia($file)
                ->withCustomProperties([
                    'type' => $type,
                    'label' => self::REQUIRED_ATTACHMENT_TYPES[$type],
                ])
                ->toMediaCollection('kontrak/addendum');
        }
    }

    private function ensureRequiredAttachmentsExist(KontrakAddendum $addendum): void
    {
        if (auth()->user()?->hasRole('admin')) {
            return;
        }

        $existingTypes = $addendum->getMedia('kontrak/addendum')
            ->map(fn ($media) => $media->getCustomProperty('type'))
            ->filter()
            ->values()
            ->all();

        $missing = collect(self::REQUIRED_ATTACHMENT_TYPES)
            ->reject(fn ($label, $type) => in_array($type, $existingTypes, true))
            ->values()
            ->all();

        abort_if(count($missing) > 0, 422, 'Lampiran wajib belum lengkap: '.implode(', ', $missing));
    }

    private function ensureEditable(KontrakAddendum $addendum): void
    {
        abort_if($addendum->status === 'disetujui', 422, 'Addendum yang sudah disetujui tidak bisa diubah');
    }

    private function authorizeAdmin(): void
    {
        abort_unless(auth()->user()?->hasRole('admin'), 403, 'Hanya admin yang boleh melakukan aksi ini');
    }

    private function authorizeViewKontrak(Kontrak $kontrak): void
    {
        if (auth()->user()?->hasRole('admin')) {
            return;
        }

        abort_unless(
            Pekerjaan::byUserRole()->whereKey($kontrak->id_pekerjaan)->exists(),
            403,
            'Anda tidak memiliki akses ke kontrak ini'
        );
    }

    private function authorizeSubmit(KontrakAddendum $addendum): void
    {
        $user = auth()->user();

        if ($user?->hasRole('admin')) {
            return;
        }

        $isPengawas = $user?->roles
            ->contains(fn ($role) => strtolower((string) $role->name) === 'pengawas');

        abort_unless($isPengawas, 403, 'Hanya admin atau Pengawas yang boleh mengajukan addendum');

        abort_unless(
            Pekerjaan::byUserRole()->whereKey($addendum->kontrak->id_pekerjaan)->exists(),
            403,
            'Pengawas hanya boleh mengajukan addendum untuk pekerjaan yang diassign'
        );
    }

    private function authorizeCreate(Kontrak $kontrak): void
    {
        $user = auth()->user();

        if ($user?->hasRole('admin')) {
            return;
        }

        $isPengawas = $user?->roles
            ->contains(fn ($role) => strtolower((string) $role->name) === 'pengawas');

        abort_unless($isPengawas, 403, 'Hanya admin atau Pengawas yang boleh membuat pengajuan addendum');

        abort_unless(
            Pekerjaan::byUserRole()->whereKey($kontrak->id_pekerjaan)->exists(),
            403,
            'Pengawas hanya boleh mengajukan addendum untuk pekerjaan yang diassign'
        );
    }
}
