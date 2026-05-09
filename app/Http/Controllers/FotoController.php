<?php

namespace App\Http\Controllers;

use App\Models\Foto;
use App\Models\Pekerjaan;
use App\Http\Resources\FotoResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class FotoController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/foto",
     *     summary="List all foto",
     *     tags={"Media Management (Foto)"},
     *     @OA\Parameter(name="tahun", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="pekerjaan_id", in="query", required=false, @OA\Schema(type="integer")),
     *     @OA\Parameter(name="latest_only", in="query", required=false, @OA\Schema(type="boolean")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index(Request $request)
    {
        $query = Foto::with(['pekerjaan', 'penerima', 'komponen']);

        if ($request->has('tahun') && $request->tahun) {
            $query->whereHas('pekerjaan.kegiatan', function($q) use ($request) {
                $q->where('tahun_anggaran', $request->tahun);
            });
        }

        if ($request->has('pekerjaan_id')) {
            $query->where('pekerjaan_id', $request->pekerjaan_id);
            // Return all photos for a specific pekerjaan (no pagination)
            $foto = $query->get();
            return FotoResource::collection($foto);
        }

        if ($request->has('latest_only') && $request->latest_only) {
            $query->whereIn('id', function ($q) {
                $q->selectRaw('MAX(id)')
                    ->from('tbl_foto')
                    ->groupBy('pekerjaan_id');
            });
        }

        // Support for custom pagination or all results
        $per_page = $request->get('per_page', 20);
        
        if ($per_page == -1) {
            $foto = $query->get();
        } else {
            $foto = $query->paginate($per_page);
        }
        
        return FotoResource::collection($foto);
    }

    /**
     * @OA\Post(
     *     path="/api/foto",
     *     summary="Upload new foto",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 required={"pekerjaan_id", "komponen_id", "keterangan", "koordinat", "file"},
     *                 @OA\Property(property="pekerjaan_id", type="integer"),
     *                 @OA\Property(property="komponen_id", type="integer"),
     *                 @OA\Property(property="penerima_id", type="integer"),
     *                 @OA\Property(property="keterangan", type="string", enum={"0%","25%","50%","75%","100%"}),
     *                 @OA\Property(property="koordinat", type="string"),
     *                 @OA\Property(property="file", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=201, description="Foto uploaded")
     * )
     */
    public function store(Request $request)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'required|exists:tbl_pekerjaan,id',
            'komponen_id' => 'required|integer',
            'penerima_id' => 'nullable|integer',
            'keterangan' => 'required|in:0%,25%,50%,75%,100%',
            'koordinat' => 'required|string|max:255',
            'validasi_koordinat' => 'boolean',
            'validasi_koordinat_message' => 'nullable|string|max:255',
            'unit_index' => 'nullable|integer',
            'file' => 'required|file|mimes:jpg,jpeg,png|max:5120', // Max 5MB and images only
        ]);

        $foto = Foto::create($validated);

        if ($request->hasFile('file')) {
            $foto->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('foto/pekerjaan');
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    /**
     * @OA\Get(
     *     path="/api/foto/{id}",
     *     summary="Get foto detail",
     *     tags={"Media Management (Foto)"},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function show(Foto $foto)
    {
        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    /**
     * @OA\Post(
     *     path="/api/foto/{id}",
     *     summary="Update foto",
     *     description="Uses POST with _method=PUT for multipart/form-data support",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Updated")
     * )
     */
    public function update(Request $request, Foto $foto)
    {
        $validated = $request->validate([
            'pekerjaan_id' => 'nullable|exists:tbl_pekerjaan,id',
            'komponen_id' => 'nullable|integer',
            'penerima_id' => 'nullable|integer',
            'keterangan' => 'nullable|in:0%,25%,50%,75%,100%',
            'koordinat' => 'nullable|string|max:255',
            'validasi_koordinat' => 'nullable|boolean',
            'validasi_koordinat_message' => 'nullable|string|max:255',
            'unit_index' => 'nullable|integer',
            'file' => 'nullable|file|mimes:jpg,jpeg,png|max:5120',
        ]);

        $foto->update($validated);

        if ($request->hasFile('file')) {
            $foto->clearMediaCollection('foto/pekerjaan');
            $foto->addMediaFromRequest('file')
                ->usingFileName(Str::uuid() . '.' . $request->file('file')->getClientOriginalExtension())
                ->toMediaCollection('foto/pekerjaan');
        }

        $foto->load(['pekerjaan', 'penerima', 'komponen']);
        return new FotoResource($foto);
    }

    /**
     * @OA\Delete(
     *     path="/api/foto/{id}",
     *     summary="Delete foto",
     *     tags={"Media Management (Foto)"},
     *     security={{"bearerAuth":{}}},
     *     @OA\Parameter(name="id", in="path", required=true, @OA\Schema(type="integer")),
     *     @OA\Response(response=200, description="Deleted")
     * )
     */
    public function destroy(Foto $foto)
    {
        // Delete all media files from storage
        $foto->clearMediaCollection('foto/pekerjaan');
        
        // Delete the database record
        $foto->delete();
        
        return response()->json(['message' => 'Foto deleted successfully']);
    }

    public function generateVideo(Pekerjaan $pekerjaan)
    {
        $pekerjaan->load(['foto.penerima', 'foto.komponen']);
        
        $fotos = $pekerjaan->foto->sortBy([
            ['keterangan', 'asc'],
            ['created_at', 'asc'],
        ]);

        if ($fotos->count() === 0) {
            return response()->json(['message' => 'Tidak ada foto untuk dibuat video'], 404);
        }

        $tempDir = storage_path('app/temp-video/' . uniqid());
        if (!file_exists($tempDir)) {
            mkdir($tempDir, 0775, true);
        }

        try {
            $ffmpeg = 'ffmpeg';
            $isWindows = strtoupper(substr(PHP_OS, 0, 3)) === 'WIN';
            
            // Set Font Path based on OS
            if ($isWindows) {
                $fontPath = 'C\\:/Windows/Fonts/arial.ttf';
            } else {
                // Ubuntu default font path
                $fontPath = '/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf';
                // Fallback check for Ubuntu
                if (!file_exists($fontPath)) {
                    $fontPath = '/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf';
                }
            }

            $namaPaket = str_replace(["'", "\""], "", $pekerjaan->nama_paket);
            
            $titleLen = strlen($namaPaket);
            $titleSize = 32;
            if ($titleLen > 30) $titleSize = 24;
            if ($titleLen > 60) $titleSize = 18;

            $i = 0;
            foreach ($fotos as $foto) {
                $media = $foto->getFirstMedia('foto/pekerjaan');
                if ($media && file_exists($media->getPath())) {
                    $inputPath = str_replace('\\', '/', $media->getPath());
                    $outputPath = str_replace('\\', '/', $tempDir . '/' . sprintf('%04d', $i) . '.jpg');
                    
                    $progressText = $foto->keterangan ?: 'Progress';
                    if (is_numeric($progressText)) $progressText .= "%";
                    $progressText = "PROGRESS: " . $progressText;

                    $safeFontPath = str_replace('\\', '/', $fontPath);
                    $safeFontPath = str_replace(':', '\\:', $safeFontPath); // Escape colon for Windows path in drawtext
                    
                    $vf = "split[bg][fg];";
                    $vf .= "[bg]scale=720:1280:force_original_aspect_ratio=increase,crop=720:1280,boxblur=20:10[bg];";
                    $vf .= "[fg]scale=720:1280:force_original_aspect_ratio=decrease[fg];";
                    $vf .= "[bg][fg]overlay=(W-w)/2:(H-h)/2,";
                    $vf .= "drawbox=y=ih-250:color=black@0.6:width=iw:height=180:t=fill,";
                    $vf .= "drawtext=fontfile='$safeFontPath':text='$namaPaket':x=(w-text_w)/2:y=h-210:fontsize=$titleSize:fontcolor=white,";
                    $vf .= "drawtext=fontfile='$safeFontPath':text='$progressText':x=(w-text_w)/2:y=h-140:fontsize=48:fontcolor=yellow";
                    
                    $cmd = "ffmpeg -y -i \"$inputPath\" -vf \"$vf\" -frames:v 1 -q:v 2 \"$outputPath\" 2>&1";
                    exec($cmd, $imageOutput, $imageReturn);
                    
                    if ($imageReturn !== 0) {
                        \Illuminate\Support\Facades\Log::error("FFmpeg Image Process Failed", [
                            'cmd' => $cmd,
                            'output' => $imageOutput,
                            'return' => $imageReturn
                        ]);
                    }
                    $i++;
                }
            }

            if ($i === 0) {
                return response()->json(['message' => 'File foto tidak ditemukan di storage'], 404);
            }

            $outputFile = str_replace('\\', '/', $tempDir . '/progress_video.mp4');
            $inputPattern = str_replace('\\', '/', $tempDir . '/%04d.jpg');
            
            $finalCmd = "ffmpeg -y -framerate 1 -i \"$inputPattern\" ";
            $finalCmd .= "-c:v libx264 -pix_fmt yuv420p -preset ultrafast -crf 23 -r 30 \"$outputFile\" 2>&1";
            
            exec($finalCmd, $output, $returnVar);

            if ($returnVar !== 0) {
                \Illuminate\Support\Facades\Log::error("FFmpeg Video Combine Failed", [
                    'cmd' => $finalCmd,
                    'output' => $output,
                    'return' => $returnVar
                ]);
                return response()->json([
                    'message' => 'Gagal membuat video portrait.',
                    'error' => $output
                ], 500);
            }

            $downloadName = Str::slug($pekerjaan->nama_paket) . '_story_progress.mp4';
            $finalPath = storage_path('app/public/' . $downloadName);
            rename($outputFile, $finalPath);

            return response()->download($finalPath, $downloadName)->deleteFileAfterSend(true);
        } finally {
            if (file_exists($tempDir)) {
                $files = glob($tempDir . '/*');
                foreach ($files as $file) {
                    if (is_file($file)) unlink($file);
                }
                rmdir($tempDir);
            }
        }
    }
}
