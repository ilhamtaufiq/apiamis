<?php

namespace App\Http\Controllers;

use App\Models\Pekerjaan;
use App\Services\KoordinatValidationService;
use Illuminate\Http\Request;

class KoordinatValidationController extends Controller
{
    public function __construct(
        private readonly KoordinatValidationService $koordinatValidationService,
    ) {}

    public function validate(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'koordinat' => 'required|string|max:255',
        ]);

        $pekerjaan = Pekerjaan::query()->findOrFail($validated['pekerjaan_id']);
        $result = $this->koordinatValidationService->validateForPekerjaan(
            $pekerjaan,
            $validated['koordinat'],
        );

        return response()->json([
            'validasi_koordinat' => $result['valid'],
            'validasi_koordinat_message' => $result['message'],
        ]);
    }
}