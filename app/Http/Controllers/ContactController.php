<?php

namespace App\Http\Controllers;

use App\Services\ContactInquiryService;
use Illuminate\Http\JsonResponse;
use Illuminate\Http\Request;

class ContactController extends Controller
{
    public function store(Request $request, ContactInquiryService $service): JsonResponse
    {
        if ($request->filled('website')) {
            return response()->json([
                'status' => 'success',
                'message' => 'Pesan Anda telah terkirim. Tim kami akan menghubungi Anda segera.',
            ]);
        }

        $validated = $request->validate([
            'name' => ['required', 'string', 'max:120'],
            'email' => ['required', 'email', 'max:255'],
            'phone' => ['nullable', 'string', 'max:30'],
            'subject' => ['required', 'string', 'max:200'],
            'message' => ['required', 'string', 'max:5000'],
        ]);

        $result = $service->send($validated);

        if (! $result['sent']) {
            return response()->json([
                'status' => 'error',
                'message' => $result['error'] ?? 'Pesan tidak dapat dikirim.',
            ], 503);
        }

        return response()->json([
            'status' => 'success',
            'message' => 'Pesan Anda telah terkirim. Tim kami akan menghubungi Anda segera.',
        ]);
    }
}