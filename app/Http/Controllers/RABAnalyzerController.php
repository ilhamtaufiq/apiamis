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
     *     path="/api/analyze-rab",
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
            'type' => 'nullable|string|in:default,mck'
        ]);

        $type = $request->get('type', 'default');

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
                $fullPath = Storage::disk('local')->path($path);
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

            // Run appropriate script based on type
            if ($type === 'mck') {
                $scriptsPath = base_path('scripts/analyze-mck.py');
                
                $pythonCmd = (PHP_OS_FAMILY === 'Windows') ? 'python' : 'python3';

                $outName = 'rab_mck_' . uniqid() . '.csv';
                $outputPath = Storage::disk('local')->path('temp-rab/' . $outName);
                $result = Process::run("$pythonCmd \"$scriptsPath\" \"$fullPath\" \"$outputPath\"");
            } else {
                // Run Node.js script
                $scriptsPath = base_path('scripts/analyze-rab.js');
                $result = Process::run("node \"$scriptsPath\" \"$fullPath\"");
            }

            if ($result->failed()) {
                Log::error('RAB Analysis failed', [
                    'type' => $type,
                    'error' => $result->errorOutput(),
                    'output' => $result->output()
                ]);
                return response()->json([
                    'success' => false,
                    'message' => 'Gagal menganalisis file: ' . $result->errorOutput()
                ], 500);
            }

            $csvOutput = trim($result->output());
            Log::info('RAB Analysis raw output: ' . $csvOutput);
            
            // Sometimes output contains other logs, let's take the last line which should be the path
            $lines = explode("\n", $csvOutput);
            $csvPath = trim(end($lines));
            Log::info('Detected CSV Path: ' . $csvPath);

            if (!file_exists($csvPath)) {
                return response()->json([
                    'success' => false,
                    'message' => 'Output CSV tidak ditemukan. Path: ' . $csvPath
                ], 500);
            }

            // Parse CSV
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
        
        // Read header to determine mapping
        $header = fgetcsv($handle);
        $mapping = [];
        foreach ($header as $index => $col) {
            $colLower = strtolower(trim($col));
            if ($colLower === 'no') $mapping['no'] = $index;
            if (str_contains($colLower, 'item') || str_contains($colLower, 'uraian')) $mapping['item'] = $index;
            if (str_contains($colLower, 'satuan')) $mapping['satuan'] = $index;
            if (str_contains($colLower, 'vol')) $mapping['vol'] = $index;
            if (str_contains($colLower, 'harga') || str_contains($colLower, 'satuan')) {
                 if (!isset($mapping['harga']) || str_contains($colLower, 'harga')) $mapping['harga'] = $index;
            }
            if (str_contains($colLower, 'pajak')) $mapping['pajak'] = $index;
            if (str_contains($colLower, 'total')) $mapping['total'] = $index;
            if (str_contains($colLower, 'keterangan')) $mapping['keterangan'] = $index;
            if (str_contains($colLower, 'kunci')) $mapping['kunci'] = $index;
            if (str_contains($colLower, 'type')) $mapping['type'] = $index;
        }
        
        while (($data = fgetcsv($handle)) !== FALSE) {
            $item = trim($data[$mapping['item'] ?? 1] ?? '');
            $no = trim($data[$mapping['no'] ?? 0] ?? '');
            
            if (($no === 'METADATA' || $no === 'METADATA_TOTAL') && $item === 'GRAND_TOTAL') {
                $documentTotal = $this->cleanNumber($data[$mapping['total'] ?? 6] ?? 0);
                continue;
            }

            $satuan = trim($data[$mapping['satuan'] ?? 2] ?? '');
            $vol = $this->cleanNumber($data[$mapping['vol'] ?? 3] ?? 0);
            $harga = $this->cleanNumber($data[$mapping['harga'] ?? 4] ?? 0);
            $total = $this->cleanNumber($data[$mapping['total'] ?? 6] ?? 0);
            $pajak = trim($data[$mapping['pajak'] ?? 5] ?? '11%');
            if (is_numeric($pajak)) $pajak = $pajak . '%';
            
            $keterangan = trim($data[$mapping['keterangan'] ?? -1] ?? '');
            $kunci = trim($data[$mapping['kunci'] ?? -1] ?? '');

            $itemLower = strtolower($item);
            $isSummary = str_contains($itemLower, 'total') || str_contains($itemLower, 'jumlah') || str_contains($itemLower, 'grand total');
            
            // Determine type from script if available, otherwise guess
            $type = 'item';
            if (isset($mapping['type'])) {
                $type = trim($data[$mapping['type']]);
            } else {
                $isHeader = ($vol == 0 && $harga == 0 && $total == 0);
                if ($isSummary) $type = 'summary';
                elseif ($isHeader) $type = 'header';
            }

            $items[] = [
                'type' => $type,
                'no' => $no,
                'item' => $item,
                'satuan' => $satuan,
                'vol' => $vol,
                'harga' => $harga,
                'pajak' => $pajak,
                'total' => $total,
                'keterangan' => $keterangan,
                'kunci' => $kunci
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
