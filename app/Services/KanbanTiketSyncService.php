<?php

namespace App\Services;

use App\Models\KanbanCard;
use App\Models\KanbanColumn;
use App\Models\Tiket;

class KanbanTiketSyncService
{
    public function syncCardToTiket(KanbanCard $card): void
    {
        if (!$card->tiket_id) {
            return;
        }

        $tiket = Tiket::find($card->tiket_id);
        if (!$tiket) {
            return;
        }

        $payload = [
            'subjek' => $card->title,
            'deskripsi' => $card->description ?? $tiket->deskripsi,
            'pekerjaan_id' => $card->pekerjaan_id,
        ];

        $column = $card->relationLoaded('column')
            ? $card->column
            : KanbanColumn::find($card->column_id);

        if ($column?->tiket_status) {
            $payload['status'] = $column->tiket_status;
        }

        $tiket->update($payload);
    }

    public function resolveColumnIdForTiketStatus(int $boardId, string $status): ?int
    {
        return KanbanColumn::query()
            ->where('board_id', $boardId)
            ->where('tiket_status', $status)
            ->value('id');
    }
}