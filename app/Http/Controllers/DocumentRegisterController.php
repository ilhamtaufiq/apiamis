<?php

namespace App\Http\Controllers;

use App\Models\DocumentRegister;
use App\Models\DocumentType;
use App\Models\Kontrak;
use Illuminate\Support\Facades\DB;
use Illuminate\Http\Request;

class DocumentRegisterController extends Controller
{
    public function index(Request $request)
    {
        $query = DocumentRegister::with(['kontrak.pekerjaan', 'kontrak.penyedia', 'type']);

        if ($request->has('tahun')) {
            $query->where('year', $request->tahun);
        }

        if ($request->has('type_id')) {
            $query->where('type_id', $request->type_id);
        }

        if ($request->has('search')) {
            $search = $request->search;
            $query->where(function($q) use ($search) {
                $q->where('nomor', 'LIKE', '%' . $search . '%')
                  ->orWhere('description', 'LIKE', '%' . $search . '%')
                  ->orWhereHas('kontrak.pekerjaan', function($sq) use ($search) {
                      $sq->where('nama_paket', 'LIKE', '%' . $search . '%')
                        ->orWhere('kode_rekening', 'LIKE', '%' . $search . '%');
                  });
            });
        }

        $perPage = $request->get('per_page', 20);
        return response()->json($query->latest()->paginate($perPage));
    }

    public function types()
    {
        return response()->json(DocumentType::all());
    }

    public function storeType(Request $request)
    {
        $validated = $request->validate([
            'name' => 'required|string',
            'code' => 'required|string|unique:tbl_document_types,code',
            'format_template' => 'nullable|string',
        ]);

        $type = DocumentType::create($validated);
        return response()->json($type, 201);
    }

    public function updateType(Request $request, $id)
    {
        $type = DocumentType::findOrFail($id);
        $validated = $request->validate([
            'name' => 'required|string',
            'code' => 'required|string|unique:tbl_document_types,code,' . $id,
            'format_template' => 'nullable|string',
        ]);

        $type->update($validated);
        return response()->json($type);
    }

    public function destroyType($id)
    {
        $type = DocumentType::findOrFail($id);
        if (DocumentRegister::where('type_id', $id)->exists()) {
            return response()->json(['message' => 'Tidak dapat menghapus tipe yang sudah memiliki data register'], 422);
        }
        $type->delete();
        return response()->json(['message' => 'Tipe berhasil dihapus']);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'kontrak_id' => 'required|exists:tbl_kontrak,id',
            'type_id' => 'required|exists:tbl_document_types,id',
            'tanggal' => 'required|date',
            'description' => 'nullable|string',
            'nilai' => 'nullable|numeric|min:0',
            'sequence_number' => 'nullable|integer|min:1',
        ]);

        $type = DocumentType::findOrFail($validated['type_id']);
        $date = new \DateTime($validated['tanggal']);
        $year = (int) $date->format('Y');

        try {
            $register = DB::transaction(function () use ($validated, $type, $date, $year, $request) {
            if ($request->filled('sequence_number')) {
                $sequence = (int) $request->sequence_number;
            } else {
                $seq = DB::table('tbl_document_sequences')
                    ->where('year', $year)
                    ->where('type', 'berita-acara')
                    ->lockForUpdate()
                    ->first();

                $sequence = $seq ? ((int) $seq->last_number + 1) : 1;
            }

            DB::table('tbl_document_sequences')
                ->updateOrInsert(
                    ['year' => $year, 'type' => 'berita-acara'],
                    ['last_number' => $sequence]
                );

            $nomor = $this->generateNumber($type, $sequence, $date, $validated['kontrak_id']);

            if (DocumentRegister::where('nomor', $nomor)->exists()) {
                throw new \RuntimeException("Nomor dokumen $nomor sudah terdaftar. Gunakan urutan manual jika ingin menggunakan nomor lain.");
            }

            return DocumentRegister::create([
                'kontrak_id' => $validated['kontrak_id'],
                'type_id' => $validated['type_id'],
                'nomor' => $nomor,
                'tanggal' => $validated['tanggal'],
                'sequence_number' => $sequence,
                'year' => $year,
                'description' => $validated['description'] ?? null,
                'nilai' => $validated['nilai'] ?? null,
            ]);
            });
        } catch (\RuntimeException $exception) {
            return response()->json(['message' => $exception->getMessage()], 422);
        }

        return response()->json($register->load('type'), 201);
    }

    public function update(Request $request, $id)
    {
        $register = DocumentRegister::findOrFail($id);
        $validated = $request->validate([
            'tanggal' => 'required|date',
            'nomor' => 'required|string|max:255',
            'description' => 'nullable|string',
            'nilai' => 'nullable|numeric|min:0',
        ]);

        if (DocumentRegister::where('nomor', $validated['nomor'])
            ->where('id', '!=', $register->id)
            ->exists()) {
            return response()->json([
                'message' => 'Nomor dokumen sudah digunakan oleh registrasi lain.',
            ], 422);
        }

        $register->update($validated);

        return response()->json($register->load('type'));
    }

    public function destroy($id)
    {
        $register = DocumentRegister::findOrFail($id);
        $year = $register->year;

        $register->delete();

        // Recalculate sequence to max remaining for that year
        $maxSeq = DocumentRegister::where('year', $year)->max('sequence_number') ?? 0;
        DB::table('tbl_document_sequences')
            ->updateOrInsert(
                ['year' => $year, 'type' => 'berita-acara'],
                ['last_number' => $maxSeq]
            );

        return response()->json(['message' => 'Register deleted']);
    }

    private function generateNumber(DocumentType $type, int $sequence, \DateTime $date, $kontrak_id = null)
    {
        $template = $type->format_template ?: '{sequence}/{code}-AMIS/{month}/{year}';
        
        $replacements = [
            '{sequence}' => str_pad($sequence, 3, '0', STR_PAD_LEFT),
            '{nomor_urut_surat}' => $sequence,
            '{code}' => $type->code,
            '{year}' => $date->format('Y'),
            '{tahun}' => $date->format('Y'),
            '{month}' => $this->getRomanMonth($date->format('n')),
            '{day}' => $date->format('d'),
            '{kontrak_id}' => $kontrak_id,
        ];

        if ($kontrak_id) {
            $kontrak = Kontrak::find($kontrak_id);
            if ($kontrak) {
                $replacements['{id_pekerjaan}'] = $kontrak->id_pekerjaan;
            }
        }

        return str_replace(array_keys($replacements), array_values($replacements), $template);
    }

    private function getRomanMonth($month)
    {
        $roman = [1 => 'I', 'II', 'III', 'IV', 'V', 'VI', 'VII', 'VIII', 'IX', 'X', 'XI', 'XII'];
        return $roman[$month] ?? $month;
    }
}
