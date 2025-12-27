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
use App\Http\Controllers\AppSettingController;
use App\Http\Controllers\BeritaAcaraController;
use App\Http\Controllers\UserPekerjaanController;
use App\Http\Controllers\TiketController;
use App\Http\Controllers\TiketCommentController;
use App\Http\Controllers\EventController;
use App\Http\Controllers\DataQualityController;
use App\Http\Controllers\AuditLogController;

// Authentication Routes
Route::post('auth/login', [AuthController::class, 'login']);

// Google OAuth Routes
Route::get('auth/google', [AuthController::class, 'redirectToGoogle']);
Route::get('auth/google/callback', [AuthController::class, 'handleGoogleCallback']);

// App Settings (public read, authenticated write)
Route::get('app-settings', [AppSettingController::class, 'index']);
Route::post('app-settings', [AppSettingController::class, 'store'])->middleware('auth:sanctum');
Route::middleware('auth:sanctum')->group(function () {
    Route::get('/', function () {
    return view('welcome');
    });
    Route::post('auth/logout', [AuthController::class, 'logout']);
    Route::get('auth/me', [AuthController::class, 'me']);
    Route::post('auth/impersonate/{user}', [AuthController::class, 'impersonate'])->middleware('role:admin');

    // Pekerjaan dengan role-based filtering
    Route::apiResource('pekerjaan', PekerjaanController::class);
    Route::get('pekerjaan/{pekerjaan}/media', [PekerjaanController::class, 'media']);
    
    // Menu permissions - user menus
    Route::get('menu-permissions/user/menus', [MenuPermissionController::class, 'getUserMenus']);
    
    // Manajemen kegiatan-role dan user-pekerjaan (hanya admin) 
    Route::middleware(['role:admin'])->group(function () {
        Route::get('kegiatan-role', [KegiatanRoleController::class, 'index']);
        Route::post('kegiatan-role', [KegiatanRoleController::class, 'store']);
        Route::delete('kegiatan-role/{kegiatanRoleId}', [KegiatanRoleController::class, 'destroy'])->where('kegiatanRoleId', '[0-9]+');
        
        // User-Pekerjaan Assignment
        Route::get('user-pekerjaan', [UserPekerjaanController::class, 'index']);
        Route::post('user-pekerjaan', [UserPekerjaanController::class, 'store']);
        Route::delete('user-pekerjaan/{id}', [UserPekerjaanController::class, 'destroy']);
        Route::get('user-pekerjaan/user/{userId}', [UserPekerjaanController::class, 'byUser']);
        Route::get('user-pekerjaan/pekerjaan/{pekerjaanId}', [UserPekerjaanController::class, 'byPekerjaan']);
        Route::get('user-pekerjaan/available-users', [UserPekerjaanController::class, 'availableUsers']);
        
        // Data Quality Diagnostic
        Route::get('data-quality/stats', [DataQualityController::class, 'getStats']);

        // Audit Logs
        Route::get('audit-logs', [AuditLogController::class, 'index']);
        Route::get('audit-logs/{auditLog}', [AuditLogController::class, 'show']);
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
    Route::post('pekerjaan/import', [PekerjaanController::class, 'import']);
    Route::get('pekerjaan/import/template', [PekerjaanController::class, 'downloadTemplate']);



    // Custom routes - Kontrak
    Route::get('kontrak/pekerjaan/{pekerjaanId}', [KontrakController::class, 'byPekerjaan']);
    Route::get('kontrak/kegiatan/{kegiatanId}', [KontrakController::class, 'byKegiatan']);
    Route::get('kontrak/penyedia/{penyediaId}', [KontrakController::class, 'byPenyedia']);

    //Output
    Route::apiResource('output', OutputController::class);

    // Tiket
    Route::apiResource('tiket', TiketController::class);
    Route::post('tiket/{tiket}/comments', [TiketCommentController::class, 'store']);

    // Custom penerima  
    Route::get('penerima/pekerjaan/{pekerjaanId}', [PenerimaController::class, 'byPekerjaan']);
    Route::get('penerima/pekerjaan/{pekerjaanId}/stats/komunal', [PenerimaController::class, 'komunalCount']);

    // Progress routes
    Route::get('progress/pekerjaan/{pekerjaanId}', [ProgressController::class, 'report']);
    Route::post('progress/pekerjaan/{pekerjaanId}', [ProgressController::class, 'store']);

    // Berita Acara routes
    Route::get('berita-acara/pekerjaan/{pekerjaanId}', [BeritaAcaraController::class, 'show']);
    Route::post('berita-acara/pekerjaan/{pekerjaanId}', [BeritaAcaraController::class, 'storeOrUpdate']);

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

    // Notifications
    Route::get('/notifications', [\App\Http\Controllers\NotificationController::class, 'index']);
    Route::post('/notifications/{id}/read', [\App\Http\Controllers\NotificationController::class, 'markAsRead']);
    Route::post('/notifications/mark-all-read', [\App\Http\Controllers\NotificationController::class, 'markAllAsRead']);
    Route::post('/notifications/broadcast', [\App\Http\Controllers\NotificationController::class, 'sendBroadcast'])->middleware('role:admin');

    // Calendar Events
    Route::apiResource('events', EventController::class);
});


