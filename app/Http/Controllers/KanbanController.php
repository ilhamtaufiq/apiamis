<?php

namespace App\Http\Controllers;

use App\Http\Resources\KanbanBoardResource;
use App\Http\Resources\KanbanCardResource;
use App\Models\KanbanBoard;
use App\Models\KanbanCard;
use App\Models\KanbanColumn;
use App\Models\Tiket;
use App\Services\KanbanTiketSyncService;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;
use Illuminate\Support\Facades\Validator;

class KanbanController extends Controller
{
    public function __construct(
        private readonly KanbanTiketSyncService $tiketSync,
    ) {}

    public function board()
    {
        $board = $this->organizationBoard();

        $board->load([
            'columns.cards' => fn ($query) => $query->orderBy('position'),
            'columns.cards.pekerjaan.kecamatan',
            'columns.cards.pekerjaan.desa',
            'columns.cards.tiket',
            'columns.cards.creator',
        ]);

        return new KanbanBoardResource($board);
    }

    public function storeCard(Request $request)
    {
        $this->ensureAdmin();

        $board = $this->organizationBoard();

        $validator = Validator::make($request->all(), [
            'column_id' => 'required|exists:tbl_kanban_columns,id',
            'title' => 'required|string|max:255',
            'description' => 'nullable|string',
            'status_label' => 'nullable|string|max:100',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'metadata' => 'nullable|array',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $column = KanbanColumn::where('board_id', $board->id)->findOrFail($request->column_id);
        $position = (int) KanbanCard::where('column_id', $column->id)->max('position') + 1;

        $card = KanbanCard::create([
            'board_id' => $board->id,
            'column_id' => $column->id,
            'position' => $position,
            'title' => $request->title,
            'description' => $request->description,
            'status_label' => $request->status_label,
            'pekerjaan_id' => $request->pekerjaan_id,
            'source' => 'manual',
            'metadata' => $request->metadata,
            'created_by' => auth()->id(),
        ]);

        return new KanbanCardResource($card->load(['pekerjaan', 'creator']));
    }

    public function importFromTiket(Request $request)
    {
        $this->ensureAdmin();

        $board = $this->organizationBoard();

        $validator = Validator::make($request->all(), [
            'tiket_id' => 'required|exists:tbl_tiket,id',
            'column_id' => 'nullable|exists:tbl_kanban_columns,id',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $tiket = Tiket::findOrFail($request->tiket_id);

        $existing = KanbanCard::where('board_id', $board->id)
            ->where('tiket_id', $tiket->id)
            ->first();

        if ($existing) {
            return response()->json(['message' => 'Tiket sudah ada di kanban'], 409);
        }

        $columnId = $request->column_id
            ?? $this->tiketSync->resolveColumnIdForTiketStatus($board->id, $tiket->status)
            ?? $board->columns()->orderBy('position')->value('id');

        $column = KanbanColumn::where('board_id', $board->id)->findOrFail($columnId);
        $position = (int) KanbanCard::where('column_id', $column->id)->max('position') + 1;

        $card = KanbanCard::create([
            'board_id' => $board->id,
            'column_id' => $column->id,
            'position' => $position,
            'title' => $tiket->subjek,
            'description' => $tiket->deskripsi,
            'status_label' => null,
            'pekerjaan_id' => $request->pekerjaan_id ?? $tiket->pekerjaan_id,
            'tiket_id' => $tiket->id,
            'source' => 'tiket',
            'metadata' => [
                'kategori' => $tiket->kategori,
                'prioritas' => $tiket->prioritas,
            ],
            'created_by' => auth()->id(),
        ]);

        return new KanbanCardResource($card->load(['pekerjaan', 'tiket', 'creator']));
    }

    public function updateCard(Request $request, KanbanCard $card)
    {
        $this->ensureAdmin();
        $this->ensureOrganizationCard($card);

        $validator = Validator::make($request->all(), [
            'title' => 'sometimes|string|max:255',
            'description' => 'nullable|string',
            'status_label' => 'nullable|string|max:100',
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'metadata' => 'nullable|array',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $card->update($request->only([
            'title',
            'description',
            'status_label',
            'pekerjaan_id',
            'metadata',
        ]));

        $card->load('column');
        $this->tiketSync->syncCardToTiket($card);

        return new KanbanCardResource($card->load(['pekerjaan', 'tiket', 'creator']));
    }

    public function moveCard(Request $request, KanbanCard $card)
    {
        $this->ensureAdmin();
        $this->ensureOrganizationCard($card);

        $validator = Validator::make($request->all(), [
            'column_id' => 'required|exists:tbl_kanban_columns,id',
            'position' => 'required|integer|min:0',
        ]);

        if ($validator->fails()) {
            return response()->json(['message' => 'Validation error', 'errors' => $validator->errors()], 422);
        }

        $board = $this->organizationBoard();
        $targetColumn = KanbanColumn::where('board_id', $board->id)->findOrFail($request->column_id);

        DB::transaction(function () use ($card, $targetColumn, $request) {
            $oldColumnId = $card->column_id;
            $newPosition = (int) $request->position;

            if ($oldColumnId === $targetColumn->id) {
                $this->reorderWithinColumn($card, $newPosition);
            } else {
                KanbanCard::where('column_id', $oldColumnId)
                    ->where('position', '>', $card->position)
                    ->decrement('position');

                KanbanCard::where('column_id', $targetColumn->id)
                    ->where('position', '>=', $newPosition)
                    ->increment('position');

                $card->update([
                    'column_id' => $targetColumn->id,
                    'position' => $newPosition,
                ]);
            }

            $card->refresh()->load('column');
            $this->tiketSync->syncCardToTiket($card);
        });

        return new KanbanCardResource($card->load(['pekerjaan', 'tiket', 'creator', 'column']));
    }

    public function destroyCard(KanbanCard $card)
    {
        $this->ensureAdmin();
        $this->ensureOrganizationCard($card);

        $columnId = $card->column_id;
        $position = $card->position;

        $card->delete();

        KanbanCard::where('column_id', $columnId)
            ->where('position', '>', $position)
            ->decrement('position');

        return response()->json(['message' => 'Kartu kanban berhasil dihapus']);
    }

    private function organizationBoard(): KanbanBoard
    {
        return KanbanBoard::where('slug', 'organisasi')->firstOrFail();
    }

    private function ensureOrganizationCard(KanbanCard $card): void
    {
        $board = $this->organizationBoard();
        if ($card->board_id !== $board->id) {
            abort(404);
        }
    }

    private function ensureAdmin(): void
    {
        abort_unless(auth()->user()?->hasRole('admin'), 403, 'Hanya admin yang boleh melakukan aksi ini');
    }

    private function reorderWithinColumn(KanbanCard $card, int $newPosition): void
    {
        $oldPosition = $card->position;

        if ($newPosition === $oldPosition) {
            return;
        }

        if ($newPosition < $oldPosition) {
            KanbanCard::where('column_id', $card->column_id)
                ->whereBetween('position', [$newPosition, $oldPosition - 1])
                ->increment('position');
        } else {
            KanbanCard::where('column_id', $card->column_id)
                ->whereBetween('position', [$oldPosition + 1, $newPosition])
                ->decrement('position');
        }

        $card->update(['position' => $newPosition]);
    }
}