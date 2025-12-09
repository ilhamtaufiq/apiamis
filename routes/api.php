<?php

use Illuminate\Http\Request;
use Illuminate\Support\Facades\Route;
use App\Http\Controllers\AuthController;
use App\Http\Controllers\KecamatanController;
use App\Http\Controllers\DesaController;
use App\Http\Controllers\PenyediaController;
use App\Http\Controllers\KegiatanController;
use App\Http\Controllers\PekerjaanController;
use App\Http\Controllers\DashboardController;
use App\Http\Controllers\KontrakController;
use App\Http\Controllers\OutputController;
use App\Http\Controllers\PenerimaController;
use App\Http\Controllers\BerkasController;
use App\Http\Controllers\FotoController;
use App\Http\Controllers\UserController;
use App\Http\Controllers\RoleController;
use App\Http\Controllers\PermissionController;
use App\Http\Controllers\KegiatanRoleController;
use App\Http\Controllers\RoutePermissionController;
use App\Http\Controllers\MenuPermissionController;
use App\Http\Controllers\ProgressController;

// Authentication Routes
Route::post('auth/login', [AuthController::class, 'login']);
Route::middleware('auth:sanctum')->group(function () {
    Route::post('auth/logout', [AuthController::class, 'logout']);
    Route::get('auth/me', [AuthController::class, 'me']);

    // Pekerjaan dengan role-based filtering
    Route::apiResource('pekerjaan', PekerjaanController::class);
    Route::get('pekerjaan/{pekerjaan}/media', [PekerjaanController::class, 'media']);
    
    // Menu permissions - user menus
    Route::get('menu-permissions/user/menus', [MenuPermissionController::class, 'getUserMenus']);
    
    // Manajemen kegiatan-role (hanya admin) 
    Route::middleware(['role:admin'])->group(function () {
        Route::get('kegiatan-role', [KegiatanRoleController::class, 'index']);
        Route::post('kegiatan-role', [KegiatanRoleController::class, 'store']);
        Route::delete('kegiatan-role/{kegiatanRoleId}', [KegiatanRoleController::class, 'destroy'])->where('kegiatanRoleId', '[0-9]+');
    });
});

Route::get('/user', function (Request $request) {
    return $request->user();
})->middleware('auth:sanctum');

// Dashboard
Route::get('dashboard/stats', [DashboardController::class, 'stats']);

// API Resources
Route::apiResource('kecamatan', KecamatanController::class);
Route::apiResource('desa', DesaController::class);
Route::apiResource('penyedia', PenyediaController::class);
Route::apiResource('kegiatan', KegiatanController::class);
Route::apiResource('kontrak', KontrakController::class);
Route::apiResource('penerima', PenerimaController::class);
Route::apiResource('berkas', BerkasController::class)->parameters(['berkas' => 'berkas']);
Route::apiResource('foto', FotoController::class);
Route::apiResource('users', UserController::class);
Route::apiResource('roles', RoleController::class);
Route::apiResource('permissions', PermissionController::class);
Route::apiResource('permissions', PermissionController::class);
Route::post('route-permissions/check-access', [RoutePermissionController::class, 'check']);
Route::get('route-permissions/rules', [RoutePermissionController::class, 'rules']);
Route::get('route-permissions/user/accessible', [RoutePermissionController::class, 'accessible']);
Route::apiResource('route-permissions', RoutePermissionController::class);
Route::apiResource('menu-permissions', MenuPermissionController::class);
// Custom routes
Route::get('desa/kecamatan/{kecamatanId}', [DesaController::class, 'byKecamatan']);
Route::get('kegiatan/tahun/{tahun}', [KegiatanController::class, 'byTahun']);

// Custom routes - Pekerjaan
Route::get('pekerjaan/kecamatan/{kecamatanId}', [PekerjaanController::class, 'byKecamatan']);
Route::get('pekerjaan/desa/{desaId}', [PekerjaanController::class, 'byDesa']);
Route::get('pekerjaan/kegiatan/{kegiatanId}', [PekerjaanController::class, 'byKegiatan']);
Route::get('pekerjaan/kecamatan/{kecamatanId}/desa/{desaId}', [PekerjaanController::class, 'byKecamatanDesa']);
Route::get('pekerjaan/stats/pagu-kecamatan/{kecamatanId}', [PekerjaanController::class, 'totalPaguByKecamatan']);
Route::get('pekerjaan/stats/pagu-kegiatan/{kegiatanId}', [PekerjaanController::class, 'totalPaguByKegiatan']);

// Custom routes - Kontrak
Route::get('kontrak/pekerjaan/{pekerjaanId}', [KontrakController::class, 'byPekerjaan']);
Route::get('kontrak/kegiatan/{kegiatanId}', [KontrakController::class, 'byKegiatan']);
Route::get('kontrak/penyedia/{penyediaId}', [KontrakController::class, 'byPenyedia']);

//Output
Route::apiResource('output', OutputController::class);

// Custom penerima  
Route::get('penerima/pekerjaan/{pekerjaanId}', [PenerimaController::class, 'byPekerjaan']);
Route::get('penerima/pekerjaan/{pekerjaanId}/stats/komunal', [PenerimaController::class, 'komunalCount']);

// Progress routes
Route::get('progress/pekerjaan/{pekerjaanId}', [ProgressController::class, 'report']);
Route::post('progress/pekerjaan/{pekerjaanId}', [ProgressController::class, 'store']);

Route::get('/debug-data', function () {
    $kegiatan = \Illuminate\Support\Facades\DB::table('tbl_kegiatan')->limit(5)->get();
    $pekerjaan = \Illuminate\Support\Facades\DB::table('tbl_pekerjaan')->limit(5)->get();
    $sumPagu = \App\Models\Kegiatan::sum('pagu');
    $pekerjaanRelation = \App\Models\Pekerjaan::with('kegiatan')->first();
    
    return response()->json([
        'kegiatan_raw' => $kegiatan,
        'pekerjaan_raw' => $pekerjaan,
        'sum_pagu_eloquent' => $sumPagu,
        'pekerjaan_relation_test' => $pekerjaanRelation
    ]);
});

