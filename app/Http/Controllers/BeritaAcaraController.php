<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\Support\Facades\DB;

class BeritaAcaraController extends Controller
{
    public function getSequence(Request $request)
    {
        $request->validate(['year' => 'required|integer|min:2020']);

        $seq = DB::table('tbl_document_sequences')
            ->where('year', $request->year)
            ->where('type', 'berita-acara')
            ->first();

        return response()->json([
            'year' => (int) $request->year,
            'last_number' => $seq ? (int) $seq->last_number : 0,
        ]);
    }

    public function updateSequence(Request $request)
    {
        $request->validate([
            'year' => 'required|integer|min:2020',
            'last_number' => 'required|integer|min:0',
        ]);

        DB::table('tbl_document_sequences')
            ->updateOrInsert(
                ['year' => $request->year, 'type' => 'berita-acara'],
                ['last_number' => $request->last_number]
            );

        return response()->json([
            'year' => (int) $request->year,
            'last_number' => (int) $request->last_number,
        ]);
    }
}
