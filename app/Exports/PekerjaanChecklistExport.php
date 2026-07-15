<?php

namespace App\Exports;

use App\Models\ChecklistItem;
use App\Models\Pekerjaan;
use App\Models\User;
use Illuminate\Support\Collection;
use Illuminate\Support\Facades\DB;
use Maatwebsite\Excel\Concerns\FromCollection;
use Maatwebsite\Excel\Concerns\ShouldAutoSize;
use Maatwebsite\Excel\Concerns\WithHeadings;
use Maatwebsite\Excel\Concerns\WithMapping;
use Maatwebsite\Excel\Concerns\WithStyles;
use PhpOffice\PhpSpreadsheet\Worksheet\Worksheet;

class PekerjaanChecklistExport implements FromCollection, WithHeadings, WithMapping, WithStyles, ShouldAutoSize
{
    protected Collection $columns;

    protected ?string $tahun;

    protected $kegiatanId;

    protected ?string $search;

    public function __construct($tahun = null, $kegiatanId = null, $search = null)
    {
        $this->tahun = $tahun;
        $this->kegiatanId = $kegiatanId;
        $this->search = $search;
        $this->columns = ChecklistItem::query()
            ->where('context', 'pekerjaan')
            ->orderBy('sort_order')
            ->get();
    }

    public function collection()
    {
        $query = Pekerjaan::with(['kegiatan'])->byUserRole();

        if ($this->tahun) {
            $query->whereHas('kegiatan', function ($q) {
                $q->where('tahun_anggaran', $this->tahun);
            });
        }

        if ($this->kegiatanId) {
            $query->where('kegiatan_id', $this->kegiatanId);
        }

        if ($this->search) {
            $query->where('nama_paket', 'LIKE', '%'.$this->search.'%');
        }

        $pekerjaan = $query->orderBy('id')->get();
        $pekerjaanIds = $pekerjaan->pluck('id')->all();

        $checklistData = DB::table('pekerjaan_checklist')
            ->whereIn('pekerjaan_id', $pekerjaanIds)
            ->get()
            ->groupBy('pekerjaan_id');

        $userIds = $checklistData->flatten()->pluck('checked_by')->filter()->unique()->values();
        $users = User::whereIn('id', $userIds)->pluck('name', 'id');

        return $pekerjaan->map(function ($p, $index) use ($checklistData, $users) {
            $rows = $checklistData->get($p->id) ?? collect();
            $latest = $rows->sortByDesc(fn ($r) => $r->updated_at ?? $r->checked_at)->first();

            $item = [
                'no' => $index + 1,
                'nama_paket' => $p->nama_paket,
                'kegiatan' => $p->kegiatan?->nama_sub_kegiatan ?? '-',
                'cells' => [],
                'tanggal_update' => $latest?->updated_at ?? $latest?->checked_at,
                'diubah_oleh' => $latest && $latest->checked_by
                    ? ($users[$latest->checked_by] ?? '-')
                    : '-',
            ];

            foreach ($this->columns as $col) {
                $data = $rows->firstWhere('checklist_item_id', $col->id);
                $checked = $data && $data->is_checked;
                $at = $data?->updated_at ?? $data?->checked_at;
                $by = $data && $data->checked_by ? ($users[$data->checked_by] ?? '-') : '-';
                $item['cells'][$col->id] = $checked
                    ? 'Ya'.($at ? ' ('.date('d/m/Y H:i', strtotime($at)).($by !== '-' ? ' · '.$by : '').')' : '')
                    : 'Tidak';
            }

            return $item;
        });
    }

    public function headings(): array
    {
        $heads = ['No', 'Nama Paket', 'Kegiatan'];
        foreach ($this->columns as $col) {
            $heads[] = $col->name;
        }
        $heads[] = 'Tanggal Update Terakhir';
        $heads[] = 'Diubah Oleh';

        return $heads;
    }

    public function map($row): array
    {
        $mapped = [
            $row['no'],
            $row['nama_paket'],
            $row['kegiatan'],
        ];

        foreach ($this->columns as $col) {
            $mapped[] = $row['cells'][$col->id] ?? 'Tidak';
        }

        $mapped[] = $row['tanggal_update']
            ? date('d/m/Y H:i', strtotime($row['tanggal_update']))
            : '-';
        $mapped[] = $row['diubah_oleh'] ?? '-';

        return $mapped;
    }

    public function styles(Worksheet $sheet)
    {
        return [
            1 => ['font' => ['bold' => true]],
        ];
    }
}
