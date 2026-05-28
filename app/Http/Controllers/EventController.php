<?php

namespace App\Http\Controllers;

use App\Models\Event;
use App\Models\Berkas;
use App\Models\DocumentRegister;
use App\Models\Kontrak;
use App\Http\Resources\EventResource;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Auth;
use Illuminate\Support\Carbon;

class EventController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/events",
     *     summary="List all events for current user",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $events = Event::where('user_id', Auth::id())->get();
        $manualEvents = EventResource::collection($events)->resolve(request());

        return response()->json([
            'data' => array_merge($manualEvents, $this->automaticEvents()),
        ]);
    }

    private function automaticEvents(): array
    {
        return array_merge(
            $this->contractEvents(),
            $this->documentRegisterEvents(),
            $this->berkasEvents(),
        );
    }

    private function contractEvents(): array
    {
        $dateFields = [
            'tanggal_penawaran' => 'Tanggal Penawaran',
            'tgl_sppbj' => 'Tanggal SPPBJ',
            'tgl_spk' => 'Tanggal SPK',
            'tgl_spmk' => 'Tanggal SPMK',
            'tgl_selesai' => 'Tanggal Selesai Kontrak',
        ];

        $events = [];

        $contracts = Kontrak::with(['pekerjaan.kecamatan', 'pekerjaan.desa', 'penyedia'])
            ->whereHas('pekerjaan', fn ($query) => $query->byUserRole())
            ->get();

        foreach ($contracts as $contract) {
            foreach ($dateFields as $field => $label) {
                if (! $contract->{$field}) {
                    continue;
                }

                $pekerjaan = $contract->pekerjaan;
                $events[] = $this->makeAutomaticEvent(
                    id: "auto:kontrak:{$contract->id}:{$field}",
                    title: "{$label}: " . ($pekerjaan?->nama_paket ?? "Kontrak #{$contract->id}"),
                    date: $contract->{$field},
                    category: 'milestone',
                    color: '#0369a1',
                    backgroundColor: '#e0f2fe',
                    borderColor: '#0284c7',
                    location: $this->formatPekerjaanLocation($pekerjaan),
                    description: collect([
                        'Sumber: Kontrak',
                        $contract->spk ? "No. SPK: {$contract->spk}" : null,
                        $contract->spmk ? "No. SPMK: {$contract->spmk}" : null,
                        $contract->sppbj ? "No. SPPBJ: {$contract->sppbj}" : null,
                        $contract->penyedia?->nama ? "Penyedia: {$contract->penyedia->nama}" : null,
                    ])->filter()->implode("\n"),
                );
            }
        }

        return $events;
    }

    private function documentRegisterEvents(): array
    {
        return DocumentRegister::with(['type', 'kontrak.pekerjaan.kecamatan', 'kontrak.pekerjaan.desa'])
            ->whereHas('kontrak.pekerjaan', fn ($query) => $query->byUserRole())
            ->whereNotNull('tanggal')
            ->get()
            ->map(fn ($register) => $this->makeAutomaticEvent(
                id: "auto:document-register:{$register->id}",
                title: 'Surat/Dokumen: ' . ($register->type?->name ?? 'Register Dokumen'),
                date: $register->tanggal,
                category: 'milestone',
                color: '#7c2d12',
                backgroundColor: '#ffedd5',
                borderColor: '#fb923c',
                location: $this->formatPekerjaanLocation($register->kontrak?->pekerjaan),
                description: collect([
                    'Sumber: Register surat/dokumen',
                    $register->nomor ? "Nomor: {$register->nomor}" : null,
                    $register->description ? "Keterangan: {$register->description}" : null,
                    $register->kontrak?->pekerjaan?->nama_paket ? "Pekerjaan: {$register->kontrak->pekerjaan->nama_paket}" : null,
                ])->filter()->implode("\n"),
            ))
            ->all();
    }

    private function berkasEvents(): array
    {
        return Berkas::with(['pekerjaan.kecamatan', 'pekerjaan.desa'])
            ->whereHas('pekerjaan', fn ($query) => $query->byUserRole())
            ->whereNotNull('created_at')
            ->get()
            ->map(fn ($berkas) => $this->makeAutomaticEvent(
                id: "auto:berkas:{$berkas->id}",
                title: 'Berkas diunggah: ' . ($berkas->jenis_dokumen ?? 'Dokumen'),
                date: $berkas->created_at,
                category: 'task',
                color: '#166534',
                backgroundColor: '#dcfce7',
                borderColor: '#22c55e',
                location: $this->formatPekerjaanLocation($berkas->pekerjaan),
                description: collect([
                    'Sumber: Berkas pekerjaan',
                    $berkas->pekerjaan?->nama_paket ? "Pekerjaan: {$berkas->pekerjaan->nama_paket}" : null,
                    $berkas->jenis_dokumen ? "Jenis dokumen: {$berkas->jenis_dokumen}" : null,
                ])->filter()->implode("\n"),
            ))
            ->all();
    }

    private function makeAutomaticEvent(
        string $id,
        string $title,
        mixed $date,
        string $category,
        string $color,
        string $backgroundColor,
        string $borderColor,
        ?string $location = null,
        ?string $description = null,
    ): array {
        $start = Carbon::parse($date)->startOfDay();
        $end = Carbon::parse($date)->endOfDay();

        return [
            'id' => $id,
            'user_id' => Auth::id(),
            'title' => $title,
            'isAllday' => true,
            'start' => $start->toISOString(),
            'end' => $end->toISOString(),
            'category' => $category,
            'location' => $location,
            'description' => $description,
            'color' => $color,
            'backgroundColor' => $backgroundColor,
            'borderColor' => $borderColor,
            'attachments' => [],
            'created_at' => null,
            'updated_at' => null,
            'isAutomatic' => true,
        ];
    }

    private function formatPekerjaanLocation($pekerjaan): ?string
    {
        if (! $pekerjaan) {
            return null;
        }

        return collect([
            $pekerjaan->desa?->n_desa,
            $pekerjaan->kecamatan?->n_kec,
        ])->filter()->implode(', ') ?: null;
    }

    /**
     * @OA\Post(
     *     path="/api/events",
     *     summary="Create new event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\JsonContent(
     *             required={"title", "start", "end"},
     *             @OA\Property(property="title", type="string"),
     *             @OA\Property(property="start", type="string", format="date-time"),
     *             @OA\Property(property="end", type="string", format="date-time"),
     *             @OA\Property(property="is_allday", type="boolean"),
     *             @OA\Property(property="category", type="string", enum={"event","task","milestone","holiday"}),
     *             @OA\Property(property="location", type="string"),
     *             @OA\Property(property="description", type="string"),
     *             @OA\Property(property="attachments", type="array", @OA\Items(type="object"))
     *         )
     *     ),
     *     @OA\Response(response=201, description="Event created")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'title' => 'required|string|max:255',
            'is_allday' => 'boolean',
            'start' => 'required|date',
            'end' => 'required|date|after_or_equal:start',
            'category' => 'string|in:event,task,milestone,holiday',
            'location' => 'nullable|string|max:255',
            'description' => 'nullable|string',
            'color' => 'nullable|string|max:20',
            'bg_color' => 'nullable|string|max:20',
            'border_color' => 'nullable|string|max:20',
            'attachments' => 'nullable|array',
        ]);

        $event = Event::create(array_merge($validated, ['user_id' => Auth::id()]));

        return new EventResource($event);
    }

    /**
     * @OA\Get(
     *     path="/api/events/{id}",
     *     summary="Get event detail",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }
        return new EventResource($event);
    }

    /**
     * @OA\Put(
     *     path="/api/events/{id}",
     *     summary="Update event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }

        $validated = $request->validate([
            'title' => 'sometimes|required|string|max:255',
            'is_allday' => 'boolean',
            'start' => 'sometimes|required|date',
            'end' => 'sometimes|required|date|after_or_equal:start',
            'category' => 'string|in:event,task,milestone,holiday',
            'location' => 'nullable|string|max:255',
            'description' => 'nullable|string',
            'color' => 'nullable|string|max:20',
            'bg_color' => 'nullable|string|max:20',
            'border_color' => 'nullable|string|max:20',
            'attachments' => 'nullable|array',
        ]);

        $event->update($validated);

        return new EventResource($event);
    }

    /**
     * @OA\Delete(
     *     path="/api/events/{id}",
     *     summary="Delete event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }

        $event->delete();

        return response()->json(['message' => 'Event deleted successfully']);
    }

    /**
     * @OA\Post(
     *     path="/api/events/{id}/upload",
     *     summary="Upload attachment for event",
     *     tags={"Calendar Events"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"file"},
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=200, description="File uploaded")
     * )
     */
    public function upload(Request $request, Event $event)
    {
        if ($event->user_id !== Auth::id()) {
            abort(403);
        }

        $request->validate([
            'file' => 'required|file|max:10240', // Max 10MB
        ]);

        $media = $event->addMediaFromRequest('file')
            ->toMediaCollection('event/attachments');

        $attachments = $event->attachments ?? [];
        $attachments[] = [
            'id' => $media->id,
            'name' => $media->file_name,
            'url' => $media->getFullUrl(),
            'type' => $media->mime_type,
            'size' => $media->size,
        ];

        $event->update(['attachments' => $attachments]);

        return new EventResource($event);
    }
}
