<?php

use App\Http\Controllers\AnalyticsController;
use App\Http\Controllers\Api\PengawasController;
use App\Http\Controllers\AppSettingController;
use App\Http\Controllers\AuditLogController;
use App\Http\Controllers\AuthController;
use App\Http\Controllers\BackupController;
use App\Http\Controllers\BerkasController;
use App\Http\Controllers\ChecklistItemController;
use App\Http\Controllers\ClientErrorReportController;
use App\Http\Controllers\DashboardController;
use App\Http\Controllers\DataQualityController;
use App\Http\Controllers\DesaController;
use App\Http\Controllers\DraftPekerjaanController;
use App\Http\Controllers\EventController;
use App\Http\Controllers\FotoController;
use App\Http\Controllers\KecamatanController;
use App\Http\Controllers\KegiatanController;
use App\Http\Controllers\KegiatanRoleController;
use App\Http\Controllers\KontrakAddendumController;
use App\Http\Controllers\KontrakController;
use App\Http\Controllers\MenuPermissionController;
use App\Http\Controllers\OutputController;
use App\Http\Controllers\PekerjaanChecklistController;
use App\Http\Controllers\PekerjaanController;
use App\Http\Controllers\PenerimaController;
use App\Http\Controllers\PenyediaController;
use App\Http\Controllers\PermissionController;
use App\Http\Controllers\ProgressController;
use App\Http\Controllers\RABAnalyzerController;
use App\Http\Controllers\RkaController;
use App\Http\Controllers\RoleController;
use App\Http\Controllers\RoutePermissionController;
use App\Http\Controllers\SimulationNetworkController;
use App\Http\Controllers\SpamUnitController;
use App\Http\Controllers\TagController;
use App\Http\Controllers\TiketCommentController;
use App\Http\Controllers\TiketController;
use App\Http\Controllers\UserController;
use App\Http\Controllers\UserPekerjaanController;
use App\Http\Controllers\WhatsAppController;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Route;

// Authentication Routes
Route::post('auth/login', [AuthController::class, 'login']);

// Google OAuth Routes
Route::get('auth/google', [AuthController::class, 'redirectToGoogle']);
Route::get('auth/google/callback', [AuthController::class, 'handleGoogleCallback']);

// App Settings (public read, authenticated write)
Route::get('app-settings', [AppSettingController::class, 'index']);
Route::get('app-settings/storage-stats', [AppSettingController::class, 'storageStats'])->middleware('auth:sanctum');
Route::post('app-settings', [AppSettingController::class, 'store'])->middleware('auth:sanctum');

// Public Blog Routes
Route::get('blog', [\App\Http\Controllers\BlogController::class, 'index']);
Route::get('blog/{blog}', [\App\Http\Controllers\BlogController::class, 'show']);

Route::middleware('auth:sanctum')->group(function () {
    Route::get('/', function () {
        return view('welcome');
    });
    Route::post('auth/logout', [AuthController::class, 'logout']);
    Route::get('auth/me', [AuthController::class, 'me']);
    Route::post('auth/impersonate/{user}', [AuthController::class, 'impersonate'])->middleware('role:admin');

    // Frontend error reporting
    Route::post('client-error-reports', [ClientErrorReportController::class, 'store']);

    // Custom routes - Pekerjaan
    Route::get('pekerjaan/document-register', [PekerjaanController::class, 'documentRegister']);
    Route::get('pekerjaan/kecamatan/{kecamatanId}', [PekerjaanController::class, 'byKecamatan']);
    Route::get('pekerjaan/desa/{desaId}', [PekerjaanController::class, 'byDesa']);
    Route::get('pekerjaan/kegiatan/{kegiatanId}', [PekerjaanController::class, 'byKegiatan']);
    Route::get('pekerjaan/kecamatan/{kecamatanId}/desa/{desaId}', [PekerjaanController::class, 'byKecamatanDesa']);
    Route::get('pekerjaan/stats/pagu-kecamatan/{kecamatanId}', [PekerjaanController::class, 'totalPaguByKecamatan']);
    Route::get('pekerjaan/stats/pagu-kegiatan/{kegiatanId}', [PekerjaanController::class, 'totalPaguByKegiatan']);
    Route::post('pekerjaan/import', [PekerjaanController::class, 'import']);
    Route::get('pekerjaan/import/template', [PekerjaanController::class, 'downloadTemplate']);

    // Pekerjaan dengan role-based filtering
    Route::apiResource('pekerjaan', PekerjaanController::class);
    Route::get('pekerjaan/{pekerjaan}/media', [PekerjaanController::class, 'media']);
    Route::get('pekerjaan/{pekerjaan}/download-all-berkas', [PekerjaanController::class, 'downloadAllBerkas']);

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
        // Frontend Error Logs
        Route::get('error-logs', [ClientErrorReportController::class, 'index']);
        Route::get('error-logs/{errorLog}', [ClientErrorReportController::class, 'show']);
        Route::post('error-logs/{errorLog}/resolve', [ClientErrorReportController::class, 'resolve']);
        Route::post('error-logs/{errorLog}/reopen', [ClientErrorReportController::class, 'reopen']);
    });

    Route::get('/user', function (Request $request) {
        return $request->user();
    })->middleware('auth:sanctum');

    // Dashboard
    Route::get('dashboard/stats', [DashboardController::class, 'stats']);
    Route::get('dashboard/analytics', [AnalyticsController::class, 'stats']);

    // Global Search
    Route::get('search', [\App\Http\Controllers\SearchController::class, 'index']);

    // API Resources
    Route::get('spam-units/stats', [SpamUnitController::class, 'stats']);
    Route::post('spam-units/{unitSpam}/achievements', [SpamUnitController::class, 'addAchievement']);
    Route::post('spam-units/{unitSpam}/budgets', [SpamUnitController::class, 'addBudget']);
    Route::delete('spam-units/{unitSpam}/budgets/{budgetId}', [SpamUnitController::class, 'deleteBudget']);
    Route::post('spam-units/import', [SpamUnitController::class, 'import']);
    Route::apiResource('spam-units', SpamUnitController::class);
    Route::apiResource('kecamatan', KecamatanController::class);
    Route::apiResource('desa', DesaController::class);
    Route::apiResource('penyedia', PenyediaController::class)->parameters(['penyedia' => 'penyedia']);
    Route::apiResource('kegiatan', KegiatanController::class);
    Route::get('rka', [RkaController::class, 'index']);
    Route::post('rka/import', [RkaController::class, 'import']);
    Route::get('rka/{rkaDocument}', [RkaController::class, 'show']);
    Route::delete('rka/{rkaDocument}', [RkaController::class, 'destroy']);
    // Custom routes - Kontrak
    Route::get('kontrak/export/excel', [KontrakController::class, 'exportExcel']);

    Route::get('kontrak/pekerjaan/{pekerjaanId}', [KontrakController::class, 'byPekerjaan']);
    Route::get('kontrak/kegiatan/{kegiatanId}', [KontrakController::class, 'byKegiatan']);
    Route::get('kontrak/penyedia/{penyediaId}', [KontrakController::class, 'byPenyedia']);
    Route::get('kontrak/{id}/export', [KontrakController::class, 'export']);
    Route::get('kontrak-addendums', [KontrakAddendumController::class, 'all']);
    Route::get('kontrak/{kontrak}/addendums', [KontrakAddendumController::class, 'index']);
    Route::post('kontrak/{kontrak}/addendums', [KontrakAddendumController::class, 'store']);

    Route::get('kontrak-addendums/{kontrakAddendum}', [KontrakAddendumController::class, 'show']);
    Route::put('kontrak-addendums/{kontrakAddendum}', [KontrakAddendumController::class, 'update']);
    Route::delete('kontrak-addendums/{kontrakAddendum}', [KontrakAddendumController::class, 'destroy']);
    Route::post('kontrak-addendums/{kontrakAddendum}/submit', [KontrakAddendumController::class, 'submit']);
    Route::post('kontrak-addendums/{kontrakAddendum}/approve', [KontrakAddendumController::class, 'approve']);
    Route::post('kontrak-addendums/{kontrakAddendum}/reject', [KontrakAddendumController::class, 'reject']);
    Route::post('kontrak-addendums/{kontrakAddendum}/upload', [KontrakAddendumController::class, 'upload']);

    Route::post('kontrak/import', [KontrakController::class, 'import']);
    Route::get('kontrak/import/template', [KontrakController::class, 'downloadTemplate']);

    Route::apiResource('kontrak', KontrakController::class);
    Route::get('kontrak/{kontrak}/export', [KontrakController::class, 'exportDoc']);
    Route::get('kontrak/{kontrak}/export-ringkasan', [KontrakController::class, 'exportRingkasan']);
    Route::get('kontrak/{kontrak}/export-cover', [KontrakController::class, 'exportCover']);
    Route::get('kontrak/{kontrak}/export-bap', [KontrakController::class, 'exportBAP']);
    Route::get('penerima/summary', [PenerimaController::class, 'summary']);
    Route::apiResource('penerima', PenerimaController::class);
    Route::apiResource('berkas', BerkasController::class)->parameters(['berkas' => 'berkas']);
    Route::post('berkas/upload-from-url', [BerkasController::class, 'uploadFromUrl']);
    Route::get('berkas/{berkas}/export-pdf', [BerkasController::class, 'convertToPdf']);
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
    Route::post('blog/upload-video', [\App\Http\Controllers\BlogController::class, 'uploadVideo']);
    Route::post('blog/{blog}/feature', [\App\Http\Controllers\BlogController::class, 'feature']);
    Route::delete('blog/{blog}/feature', [\App\Http\Controllers\BlogController::class, 'unfeature']);
    Route::apiResource('blog', \App\Http\Controllers\BlogController::class)->except(['index', 'show']);
    Route::apiResource('tags', TagController::class);
    Route::get('pengawas/statistics', [PengawasController::class, 'statistics']);
    Route::apiResource('pengawas', PengawasController::class);
    Route::get('draft-pekerjaan/export/excel', [DraftPekerjaanController::class, 'exportExcel']);
    Route::apiResource('draft-pekerjaan', DraftPekerjaanController::class);

    // Checklist Items (column management)
    Route::apiResource('checklist-items', ChecklistItemController::class);
    Route::post('checklist-items/reorder', [ChecklistItemController::class, 'reorder']);

    // Pekerjaan Checklist
    Route::get('pekerjaan-checklist', [PekerjaanChecklistController::class, 'index']);
    Route::post('pekerjaan-checklist/toggle', [PekerjaanChecklistController::class, 'toggle']);

    // Custom routes
    Route::get('desa/kecamatan/{kecamatanId}', [DesaController::class, 'byKecamatan']);
    Route::get('kegiatan/tahun/{tahun}', [KegiatanController::class, 'byTahun']);

    // Output
    Route::get('output/summary', [OutputController::class, 'summary']);
    Route::apiResource('output', OutputController::class);

    // Tiket
    Route::post('tiket/bulk-update', [TiketController::class, 'bulkUpdate']);
    Route::apiResource('tiket', TiketController::class);
    Route::post('tiket/{tiket}/comments', [TiketCommentController::class, 'store']);

    // Custom penerima
    Route::get('penerima/pekerjaan/{pekerjaanId}', [PenerimaController::class, 'byPekerjaan']);
    Route::get('penerima/pekerjaan/{pekerjaanId}/stats/komunal', [PenerimaController::class, 'komunalCount']);

    // Progress routes
    Route::get('progress/pekerjaan/{pekerjaanId}', [ProgressController::class, 'report']);
    Route::post('progress/pekerjaan/{pekerjaanId}', [ProgressController::class, 'store']);

    // Master Fase Pekerjaan
    Route::apiResource('master-fase-pekerjaan', \App\Http\Controllers\MasterFasePekerjaanController::class);

    // Document Register (Dynamic)
    Route::get('document-types', [\App\Http\Controllers\DocumentRegisterController::class, 'types']);
    Route::post('document-types', [\App\Http\Controllers\DocumentRegisterController::class, 'storeType']);
    Route::put('document-types/{id}', [\App\Http\Controllers\DocumentRegisterController::class, 'updateType']);
    Route::delete('document-types/{id}', [\App\Http\Controllers\DocumentRegisterController::class, 'destroyType']);
    Route::get('document-registers', [\App\Http\Controllers\DocumentRegisterController::class, 'index']);
    Route::post('document-registers', [\App\Http\Controllers\DocumentRegisterController::class, 'store']);
    Route::put('document-registers/{id}', [\App\Http\Controllers\DocumentRegisterController::class, 'update']);
    Route::delete('document-registers/{id}', [\App\Http\Controllers\DocumentRegisterController::class, 'destroy']);

    Route::get('/debug-data', function () {
        $kegiatan = \Illuminate\Support\Facades\DB::table('tbl_kegiatan')->limit(5)->get();
        $pekerjaan = \Illuminate\Support\Facades\DB::table('tbl_pekerjaan')->limit(5)->get();
        $sumPagu = \App\Models\Kegiatan::sum('pagu');
        $pekerjaanRelation = \App\Models\Pekerjaan::with('kegiatan')->first();

        return response()->json([
            'kegiatan_raw' => $kegiatan,
            'pekerjaan_raw' => $pekerjaan,
            'sum_pagu_eloquent' => $sumPagu,
            'pekerjaan_relation_test' => $pekerjaanRelation,
        ]);
    });

    // WhatsApp bridge
    Route::prefix('whatsapp')
        ->middleware('role:admin')
        ->group(function () {
            Route::get('status', [WhatsAppController::class, 'status']);
            Route::post('start', [WhatsAppController::class, 'start']);
            Route::post('stop', [WhatsAppController::class, 'stop']);
            Route::post('send', [WhatsAppController::class, 'send']);
            Route::post('send-bulk', [WhatsAppController::class, 'sendBulk']);
        });

    // Notifications
    Route::get('/notifications', [\App\Http\Controllers\NotificationController::class, 'index']);
    Route::post('/notifications/{id}/read', [\App\Http\Controllers\NotificationController::class, 'markAsRead']);
    Route::post('/notifications/mark-all-read', [\App\Http\Controllers\NotificationController::class, 'markAllAsRead']);
    Route::post('/notifications/broadcast', [\App\Http\Controllers\NotificationController::class, 'sendBroadcast'])->middleware('role:admin');
    Route::get('/notifications/broadcast-history', [\App\Http\Controllers\NotificationController::class, 'getBroadcastHistory'])->middleware('role:admin');
    Route::delete('/notifications/broadcast/{id}', [\App\Http\Controllers\NotificationController::class, 'deleteBroadcast'])->middleware('role:admin');

    // Calendar Events
    Route::post('events/{event}/upload', [EventController::class, 'upload']);
    Route::apiResource('events', EventController::class);

    // Simulation Networks
    Route::apiResource('simulation-networks', SimulationNetworkController::class);
    Route::get('simulation-networks/{id}/versions', [SimulationNetworkController::class, 'versions']);
    Route::get('simulation-networks/{id}/versions/{version}', [SimulationNetworkController::class, 'showVersion']);
    Route::post('simulation-networks/{id}/versions/{version}/restore', [SimulationNetworkController::class, 'restoreVersion']);
    Route::post('simulation-networks/{id}/results', [SimulationNetworkController::class, 'saveResults']);
    Route::post('simulation-networks/{id}/duplicate', [SimulationNetworkController::class, 'duplicate']);
    Route::get('simulation-networks/pekerjaan/{pekerjaanId}', [SimulationNetworkController::class, 'byPekerjaan']);

    // System backup and restore
    Route::prefix('app-settings/backups')
        ->middleware('role:admin')
        ->group(function () {
            Route::get('/', [BackupController::class, 'index']);
            Route::post('/', [BackupController::class, 'store']);
            Route::get('jobs/{jobId}', [BackupController::class, 'showJob']);
            Route::get('{filename}', [BackupController::class, 'download'])->where('filename', '.*\.zip');
            Route::delete('{filename}', [BackupController::class, 'destroy'])->where('filename', '.*\.zip');
            Route::post('restore', [BackupController::class, 'restore']);
        });

    // RAB Analysis
    Route::post('analyze-rab', [RABAnalyzerController::class, 'analyze']);

    // Chat AI (with sessions, cache, and learning)
    Route::post('chat', [\App\Http\Controllers\ChatController::class, 'chat']);
    Route::get('chat/sessions', [\App\Http\Controllers\ChatController::class, 'sessions']);
    Route::post('chat/sessions', [\App\Http\Controllers\ChatController::class, 'createSession']);
    Route::delete('chat/sessions/{id}', [\App\Http\Controllers\ChatController::class, 'deleteSession']);
    Route::get('chat/sessions/{id}/messages', [\App\Http\Controllers\ChatController::class, 'sessionMessages']);

});
