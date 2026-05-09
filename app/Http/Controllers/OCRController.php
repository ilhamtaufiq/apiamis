<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\Support\Facades\Storage;
use Symfony\Component\Process\Process;
use Symfony\Component\Process\Exception\ProcessFailedException;

class OCRController extends Controller
{
    public function processKtp(Request $request)
    {
        $request->validate([
            'file' => 'required|image|max:10240', // 10MB limit
        ]);

        $file = $request->file('file');
        $extension = $file->getClientOriginalExtension();
        $filename = 'ocr_' . time() . '_' . uniqid() . '.' . $extension;
        
        // Save to temporary storage
        $path = $file->storeAs('temp', $filename);
        $absolutePath = storage_path('app/' . $path);

        $scriptPath = base_path('scripts/ocr_ktp.py');
        $pythonPath = base_path('scripts/venv/Scripts/python.exe');

        // Check if python venv exists
        if (!file_exists($pythonPath)) {
            // Fallback for different environments (linux/windows)
            $pythonPath = base_path('scripts/venv/bin/python');
            if (!file_exists($pythonPath)) {
                $pythonPath = 'python'; // Hope it's in path
            }
        }

        try {
            $process = new Process([$pythonPath, $scriptPath, $absolutePath]);
            $process->setTimeout(60); // OCR can be slow
            $process->run();

            // Delete temp file immediately
            Storage::delete($path);

            if (!$process->isSuccessful()) {
                return response()->json([
                    'success' => false,
                    'message' => 'OCR processing failed',
                    'error' => $process->getErrorOutput()
                ], 500);
            }

            $output = json_decode($process->getOutput(), true);

            if (isset($output['error'])) {
                return response()->json([
                    'success' => false,
                    'message' => $output['error']
                ], 422);
            }

            return response()->json([
                'success' => true,
                'data' => $output
            ]);

        } catch (\Exception $e) {
            Storage::delete($path);
            return response()->json([
                'success' => false,
                'message' => 'Exception occurred: ' . $e->getMessage()
            ], 500);
        }
    }
}
