<?php

namespace App\Http\Controllers;

use App\Models\Output;
use App\Http\Resources\OutputResource;
use Illuminate\Http\Request;

class OutputController extends Controller
{
    public function index(Request $request)
    {
        $query = Output::with('pekerjaan');

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
        }

        $output = $query->paginate(20);
        return OutputResource::collection($output);
    }

    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|integer|exists:tbl_pekerjaan,id',
            'komponen' => 'required|string|max:255',
            'satuan' => 'required|string|max:255',
            'volume' => 'required|numeric|min:0',
            'penerima_is_optional' => 'boolean',
        ]);

        $output = Output::create($validated);
        $output->load('pekerjaan');
        return new OutputResource($output);
    }

    public function show(Output $output)
    {
        $output->load('pekerjaan');
        return new OutputResource($output);
    }

    public function update(Request $request, Output $output)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|integer|exists:tbl_pekerjaan,id',
            'komponen' => 'nullable|string|max:255',
            'satuan' => 'nullable|string|max:255',
            'volume' => 'nullable|numeric|min:0',
            'penerima_is_optional' => 'nullable|boolean',
        ]);

        $output->update($validated);
        $output->load('pekerjaan');
        return new OutputResource($output);
    }

    public function destroy(Output $output)
    {
        $output->delete();
        return response()->json(['message' => 'Output deleted successfully'], 200);
    }
}
