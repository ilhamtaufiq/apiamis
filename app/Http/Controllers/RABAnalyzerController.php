<?php

namespace App\Http\Controllers;

use Illuminate\Http\Request;
use Illuminate\Support\Facades\Process;
use Illuminate\Support\Facades\Storage;
use Illuminate\Support\Facades\Log;

class RABAnalyzerController extends Controller
{
    /**
     * @OA\Post(
     *     path="/api/rab/analyze",
     *     summary="Analyze RAB document (PDF/Excel)",
     *     description="Upload a file or provide berkas_id to analyze RAB structure using Node.js script",
     *     tags={"RAB Analysis"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 @OA\Property(property="file", type="string", format="binary"),
     *                 @OA\Property(property="berkas_id", type="integer")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=200, description="Analysis result")
     * )
     */
    public function analyze(Request $request)
    {
        $request->validate([
            'file' => 'required_without:berkas_id|file|mimes:pdf,xlsx,xls',
            'berkas_id' => 'required_without:file|integer|exists:tbl_berkas,id',
        ]);

        try {
            if ($request->has('berkas_id')) {
                $berkas = \App\Models\Berkas::findOrFail($request->berkas_id);
                $media = $berkas->getFirstMedia('berkas/dokumen');
                if (!$media) {
                    return response()->json([
                        'success' => false,
                        'message' => 'Berkas tidak memiliki file fisik.'
                    ], 404);
                }
                $fullPath = $media->getPath();
                $isTemp = false;
            } else {
                $file = $request->file('file');
                $originalName = $file->getClientOriginalName();
                $filename = time() . '_' . $originalName;
                
                // Store in 'local' disk (storage/app/private as per config)
                $path = $file->storeAs('temp-rab', $filename, 'local');
                $fullPath = \Illuminate\Support\Facades\Storage::disk('local')->path($path);
                $isTemp = true;
            }

            if (!file_exists($fullPath)) {
                return response()->json([
                    'success' => false,
                    'message' => 'File tidak ditemukan di server: ' . $fullPath
                ], 404);
            }

            if (!Storage::disk('local')->exists('temp-rab')) {
                Storage::disk('local')->makeDirectory('temp-rab');
            }

            // Run Node.js script
            $scriptsPath = base_path('scripts/analyze-rab.js');
            $result = Process::run("node \"$scriptsPath\" \"$fullPath\"");

            if ($result->failed()) {
                Log::error('RAB Analysis failed', [
                    'error' => $result->errorOutput(),
                    'output' => $result->output()
                ]);
                return response()->json([
                    'success' => false,
                    'message' => 'Gagal menganalisis file: ' . $result->errorOutput()
                ], 500);
            }

            $csvOutput = trim($result->output());
            
            // Sometimes output contains other logs, let's take the last line which should be the path
            $lines = explode("\n", $csvOutput);
            $csvPath = trim(end($lines));

            if (!file_exists($csvPath)) {
                return response()->json([
                    'success' => false,
                    'message' => 'Output CSV tidak ditemukan.'
                ], 500);
            }

            // Parse CSV
            Log::info('RAB Analysis raw output: ' . $csvOutput);
            $analysis = $this->parseRabCsv($csvPath);
            Log::info('RAB Analysis result metadata', [
                'extractedTotal' => $analysis['extractedTotal'],
                'documentTotal' => $analysis['documentTotal']
            ]);

            // Cleanup
            if ($isTemp) {
                @unlink($fullPath);
            }
            @unlink($csvPath);

            return response()->json([
                'success' => true,
                'data' => $analysis
            ]);

        } catch (\Exception $e) {
            Log::error('RAB Analysis exception: ' . $e->getMessage());
            return response()->json([
                'success' => false,
                'message' => 'Terjadi kesalahan sistem: ' . $e->getMessage()
            ], 500);
        }
    }

    private function parseRabCsv($path)
    {
        $items = [];
        $documentTotal = 0;
        $handle = fopen($path, "r");
        
        // Skip header
        fgetcsv($handle);
        
        while (($data = fgetcsv($handle)) !== FALSE) {
            if (count($data) < 7) continue;
            
            $no = trim($data[0] ?? '');
            $item = trim($data[1] ?? '');
            
            if ($no === 'METADATA' && $item === 'GRAND_TOTAL') {
                $documentTotal = $this->cleanNumber($data[6] ?? 0);
                continue;
            }

            $satuan = trim($data[2] ?? '');
            $vol = $this->cleanNumber($data[3] ?? 0);
            $harga = $this->cleanNumber($data[4] ?? 0);
            $total = $this->cleanNumber($data[6] ?? 0);

            $itemLower = strtolower($item);
            $isSummary = str_contains($itemLower, 'total') || str_contains($itemLower, 'jumlah') || str_contains($itemLower, 'grand total');
            $isHeader = ($vol == 0 && $harga == 0 && $total == 0);
            
            $type = 'item';
            if ($isSummary) $type = 'summary';
            elseif ($isHeader) $type = 'header';

            $items[] = [
                'type' => $type,
                'no' => $no,
                'item' => $item,
                'satuan' => $satuan,
                'vol' => $vol,
                'harga' => $harga,
                'pajak' => '11%',
                'total' => $total
            ];
        }
        fclose($handle);

        $extractedTotal = array_sum(array_column(array_filter($items, fn($i) => $i['type'] === 'item'), 'total'));

        return [
            'items' => $items,
            'extractedTotal' => $extractedTotal,
            'documentTotal' => $documentTotal,
            'difference' => abs($extractedTotal - $documentTotal)
        ];
    }

    private function cleanNumber($val)
    {
        if (is_numeric($val)) return (float)$val;
        if (!$val) return 0;

        // Remove whitespace and currency symbols
        $val = trim($val);
        $val = preg_replace('/[^\d.,\-]/', '', $val);

        // Detect format: 1.234,56 (ID) vs 1,234.56 (EN)
        $dotPos = strrpos($val, '.');
        $commaPos = strrpos($val, ',');

        if ($dotPos !== false && $commaPos !== false) {
            if ($dotPos < $commaPos) {
                // ID format: 1.234,56
                $val = str_replace('.', '', $val);
                $val = str_replace(',', '.', $val);
            } else {
                // EN format: 1,234.56
                $val = str_replace(',', '', $val);
            }
        } elseif ($commaPos !== false) {
            // Only comma: 1,234 or 1234,56
            // If it's the last character or followed by exactly 2 digits, it might be decimal
            // But let's check if there are other commas
            if (substr_count($val, ',') > 1) {
                // Thousands
                $val = str_replace(',', '', $val);
            } else {
                // Probably decimal (ID)
                $val = str_replace(',', '.', $val);
            }
        } elseif ($dotPos !== false) {
            // Only dot: 1.234 or 1234.56
            if (substr_count($val, '.') > 1) {
                // Thousands
                $val = str_replace('.', '', $val);
            }
            // else: standard float or thousands, keep as is for float
        }

        return (float)$val;
    }
}
