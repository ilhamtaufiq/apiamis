<?php

namespace App\Http\Controllers;

use App\Models\AppSetting;
use App\Http\Resources\AppSettingResource;
use Illuminate\Http\Request;
use Illuminate\Support\Str;

class AppSettingController extends Controller
{
    /**
     * @OA\Get(
     *     path="/api/settings",
     *     summary="Get all application settings",
     *     tags={"Settings"},
     *     @OA\Response(response=200, description="Successful operation")
     * )
     */
    public function index()
    {
        $settings = AppSetting::all();
        return AppSettingResource::collection($settings);
    }

    /**
     * @OA\Post(
     *     path="/api/settings",
     *     summary="Update application settings",
     *     description="Handles text values and file uploads (logo, favicon)",
     *     tags={"Settings"},
     *     security={{"bearerAuth":{}}, {"apiKeyAuth":{}}},
     *     @OA\RequestBody(
     *         required=true,
     *         @OA\MediaType(
     *             mediaType="multipart/form-data",
     *             @OA\Schema(
     *                 @OA\Property(property="app_name", type="string"),
     *                 @OA\Property(property="app_description", type="string"),
     *                 @OA\Property(property="tahun_anggaran", type="string"),
     *                 @OA\Property(property="logo", type="string", format="binary"),
     *                 @OA\Property(property="favicon", type="string", format="binary")
     *             )
     *         )
     *     ),
     *     @OA\Response(response=200, description="Settings updated")
     * )
     */
    public function store(Request $request)
    {
        $request->validate([
            'app_name' => 'nullable|string|max:255',
            'app_description' => 'nullable|string|max:500',
            'tahun_anggaran' => 'nullable|string|max:4',
            'logo' => 'nullable|file|mimes:jpg,jpeg,png,svg|max:2048',
            'favicon' => 'nullable|file|mimes:jpg,jpeg,png,svg,ico|max:1024',
        ]);

        $updatedSettings = [];

        // Handle text settings
        if ($request->has('app_name')) {
            $setting = AppSetting::setValue('app_name', $request->app_name, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('app_description')) {
            $setting = AppSetting::setValue('app_description', $request->app_description, 'text');
            $updatedSettings[] = $setting;
        }

        if ($request->has('tahun_anggaran')) {
            $setting = AppSetting::setValue('tahun_anggaran', $request->tahun_anggaran, 'text');
            $updatedSettings[] = $setting;
        }

        // Handle file uploads
        if ($request->hasFile('logo')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'logo'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('logo')
                ->usingFileName('logo_' . Str::uuid() . '.' . $request->file('logo')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        if ($request->hasFile('favicon')) {
            $setting = AppSetting::updateOrCreate(
                ['key' => 'favicon'],
                ['type' => 'file', 'value' => null]
            );
            $setting->clearMediaCollection('app-settings');
            $setting->addMediaFromRequest('favicon')
                ->usingFileName('favicon_' . Str::uuid() . '.' . $request->file('favicon')->getClientOriginalExtension())
                ->toMediaCollection('app-settings');
            $updatedSettings[] = $setting->fresh();
        }

        // Return all settings
        $allSettings = AppSetting::all();
        return AppSettingResource::collection($allSettings);
    }

    /**
     * Get storage statistics (sizes of photos, files, and database)
     */
    public function storageStats()
    {
        // Use database summation for media sizes (much more accurate for Spatie Media Library)
        $fotoSize = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'foto/pekerjaan')
            ->sum('size') ?? 0;
            
        $fotoCount = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'foto/pekerjaan')
            ->count();
            
        $berkasSize = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'berkas/dokumen')
            ->sum('size') ?? 0;

        $berkasCount = \Illuminate\Support\Facades\DB::table('media')
            ->where('collection_name', 'berkas/dokumen')
            ->count();

        // Database size (MySQL)
        $dbName = config('database.connections.mysql.database');
        $dbSize = 0;
        try {
            $dbSizeResult = \Illuminate\Support\Facades\DB::select("
                SELECT SUM(data_length + index_length) AS size 
                FROM information_schema.TABLES 
                WHERE table_schema = ?
            ", [$dbName]);
            $dbSize = (float)($dbSizeResult[0]->size ?? 0);
        } catch (\Exception $e) {
            // Fallback for other DBs if needed
        }

        return response()->json([
            'data' => [
                'foto' => (float)$fotoSize,
                'foto_count' => $fotoCount,
                'berkas' => (float)$berkasSize,
                'berkas_count' => $berkasCount,
                'database' => $dbSize,
                'media_total' => (float)($fotoSize + $berkasSize),
                'app_total' => (float)($fotoSize + $berkasSize + $dbSize)
            ]
        ]);
    }

    private function getDirSize($directory)
    {
        if (!file_exists($directory) || !is_dir($directory)) return 0;
        
        $size = 0;
        try {
            $files = new \RecursiveIteratorIterator(
                new \RecursiveDirectoryIterator($directory, \RecursiveDirectoryIterator::SKIP_DOTS)
            );
            foreach ($files as $file) {
                $size += $file->getSize();
            }
        } catch (\Exception $e) {
            // Handle inaccessible files/directories
        }
        return $size;
    }
}
