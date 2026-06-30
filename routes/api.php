<?php

use App\Http\Controllers\AnalyticsController;
use App\Http\Controllers\Api\PengawasController;
use App\Http\Controllers\AppSettingController;
use App\Http\Controllers\AuditLogController;
use App\Http\Controllers\AuthController;
use App\Http\Controllers\BackupController;
use App\Http\Controllers\BeritaAcaraController;
use App\Http\Controllers\BerkasController;
use App\Http\Controllers\ChecklistItemController;
use App\Http\Controllers\ClientErrorReportController;
use App\Http\Controllers\ContactController;
use App\Http\Controllers\DashboardController;
use App\Http\Controllers\DataQualityController;
use App\Http\Controllers\DesaController;
use App\Http\Controllers\DraftPekerjaanController;
use App\Http\Controllers\EventController;
use App\Http\Controllers\FotoController;
use App\Http\Controllers\KoordinatValidationController;
use App\Http\Controllers\KecamatanController;
use App\Http\Controllers\KegiatanController;
use App\Http\Controllers\KanbanController;
use App\Http\Controllers\KegiatanRoleController;
use App\Http\Controllers\KontrakAddendumController;
use App\Http\Controllers\KontrakController;
use App\Http\Controllers\MenuPermissionController;
use App\Http\Controllers\OnlyOfficeController;
use App\Http\Controllers\OutputController;
use App\Http\Controllers\PekerjaanChecklistController;
use App\Http\Controllers\PekerjaanController;
use App\Http\Controllers\PekerjaanProgressEstimasiController;
use App\Http\Controllers\PenerimaController;
use App\Http\Controllers\PenyediaController;
use App\Http\Controllers\PermissionController;
use App\Http\Controllers\PostPekerjaanChecklistController;
use App\Http\Controllers\ProgressController;
use App\Http\Controllers\PuspenProgressFisikController;
use App\Http\Controllers\PuspenMediaShareController;
use App\Http\Controllers\PuspenPengawasKpiController;
use App\Http\Controllers\PuspenReviewNoteController;
use App\Http\Controllers\RoleController;
use App\Http\Controllers\RoutePermissionController;
use App\Http\Controllers\SignatureLibraryController;
use App\Http\Controllers\SimulationNetworkController;
use App\Http\Controllers\SpamUnitController;
use App\Http\Controllers\SpmSanitasiController;
use App\Http\Controllers\TagController;
use App\Http\Controllers\ToolPdfController;
use App\Http\Controllers\BlogCommentController;
use App\Http\Controllers\TiketCommentController;
use App\Http\Controllers\TiketController;
use App\Http\Controllers\UserController;
use App\Http\Controllers\UserPekerjaanController;
use App\Http\Controllers\UserPresenceController;
use Illuminate\Http\Request;
use Illuminate\Support\Facades\Route;

// Authentication Routes
Route::post('auth/login', [AuthController::class, 'login'])->middleware('throttle:login');
Route::post('auth/handoff', [AuthController::class, 'createHandoff'])->middleware(['auth:sanctum', 'throttle:10,1']);
Route::post('auth/handoff/exchange', [AuthController::class, 'exchangeHandoff'])->middleware('throttle:handoff-exchange');

// Google OAuth Routes
Route::get('auth/google', [AuthController::class, 'redirectToGoogle']);
Route::get('auth/google/callback', [AuthController::class, 'handleGoogleCallback']);

// App Settings (public read, authenticated write)
Route::get('app-settings', [AppSettingController::class, 'index']);
Route::get('app-settings/storage-stats', [AppSettingController::class, 'storageStats'])->middleware(['auth:sanctum', 'role:admin']);
Route::post('app-settings', [AppSettingController::class, 'store'])->middleware(['auth:sanctum', 'role:admin']);
Route::post('app-settings/test-ai-connection', [AppSettingController::class, 'testAiConnection'])->middleware(['auth:sanctum', 'role:admin']);
Route::post('app-settings/test-mail-connection', [AppSettingController::class, 'testMailConnection'])->middleware(['auth:sanctum', 'role:admin']);
Route::get('app-settings/mail-templates', [AppSettingController::class, 'mailTemplates'])->middleware(['auth:sanctum', 'role:admin']);
Route::post('app-settings/mail-templates', [AppSettingController::class, 'storeMailTemplates'])->middleware(['auth:sanctum', 'role:admin']);
Route::post('app-settings/mail-templates/{key}/test', [AppSettingController::class, 'testMailTemplate'])->middleware(['auth:sanctum', 'role:admin']);
Route::get('app-settings/kontrak-templates', [AppSettingController::class, 'kontrakTemplates'])->middleware(['auth:sanctum', 'role:admin']);
Route::get('app-settings/kontrak-templates/{key}/download', [AppSettingController::class, 'downloadKontrakTemplate'])->middleware(['auth:sanctum', 'role:admin']);

// Public Blog Routes
Route::get('blog', [\App\Http\Controllers\BlogController::class, 'index']);
Route::get('blog/comments', [BlogCommentController::class, 'adminIndex'])->middleware('auth:sanctum');
Route::get('blog/{blog}', [\App\Http\Controllers\BlogController::class, 'show']);
Route::get('blog/{blog}/comments', [BlogCommentController::class, 'index']);
Route::get('blog/{blog}/comments/thread/{comment}', [BlogCommentController::class, 'thread']);
Route::get('blog/{blog}/comments/count', [BlogCommentController::class, 'count']);
Route::get('public/puspen/progress-fisik', [PuspenProgressFisikController::class, 'publicIndex']);
Route::get('public/spam-units/stats', [SpamUnitController::class, 'publicStats']);
Route::get('public/spam-units/map-stats', [SpamUnitController::class, 'publicMapStats']);
Route::get('public/spm-sanitasi/stats', [SpmSanitasiController::class, 'publicStats']);
Route::get('public/spm-sanitasi/map-stats', [SpmSanitasiController::class, 'publicMapStats']);
Route::post('public/contact', [ContactController::class, 'store'])->middleware('throttle:contact-inquiries');

Route::get('public/puspen/media-shares/{shareToken}', [PuspenMediaShareController::class, 'publicShow']);
Route::get('public/puspen/media-shares/{shareToken}/preview/{media}', [PuspenMediaShareController::class, 'publicPreview']);
Route::get('public/puspen/media-shares/{shareToken}/download', [PuspenMediaShareController::class, 'publicDownload']);

// ONLYOFFICE Document Server (public — signed download & save callback)
Route::post('onlyoffice/callback', [OnlyOfficeController::class, 'callback'])->name('onlyoffice.callback');
Route::get('onlyoffice/media/{media}/download', [OnlyOfficeController::class, 'download'])->name('onlyoffice.media.download');

Route::middleware('auth:sanctum')->group(function () {
    Route::get('/', function () {
        return view('welcome');
    });
    Route::post('auth/logout', [AuthController::class, 'logout']);
    Route::get('auth/me', [AuthController::class, 'me']);
    Route::post('auth/impersonate/{user}', [AuthController::class, 'impersonate'])->middleware('role:admin');

    // Frontend error reporting
    Route::post('client-error-reports', [ClientErrorReportController::class, 'store']);

    // Berita Acara sequence
    Route::get('berita-acara/sequence', [BeritaAcaraController::class, 'getSequence']);
    Route::post('berita-acara/sequence', [BeritaAcaraController::class, 'updateSequence']);

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
    Route::post('pekerjaan/{pekerjaan}/berkas/quick-share', [BerkasController::class, 'quickShareForPekerjaan']);

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
        Route::get('user-pekerjaan/completeness-gaps', [UserPekerjaanController::class, 'completenessGaps']);
        Route::post('user-pekerjaan/broadcast-reminders', [UserPekerjaanController::class, 'broadcastReminders']);

        // Data Quality Diagnostic
        Route::get('data-quality/stats', [DataQualityController::class, 'getStats']);

        // Audit Logs
        Route::get('audit-logs', [AuditLogController::class, 'index']);
        Route::get('audit-logs/{auditLog}', [AuditLogController::class, 'show']);
        // Frontend Error Logs
        Route::get('error-logs', [ClientErrorReportController::class, 'index']);
        Route::post('error-logs/bulk/resolve', [ClientErrorReportController::class, 'bulkResolve']);
        Route::post('error-logs/bulk/reopen', [ClientErrorReportController::class, 'bulkReopen']);
        Route::post('error-logs/bulk/delete', [ClientErrorReportController::class, 'bulkDestroy']);
        Route::post('error-logs/empty', [ClientErrorReportController::class, 'destroyAll']);
        Route::delete('error-logs/bulk', [ClientErrorReportController::class, 'bulkDestroy']);
        Route::delete('error-logs/empty', [ClientErrorReportController::class, 'destroyAll']);
        Route::get('error-logs/{errorLog}', [ClientErrorReportController::class, 'show'])->whereNumber('errorLog');
        Route::post('error-logs/{errorLog}/resolve', [ClientErrorReportController::class, 'resolve'])->whereNumber('errorLog');
        Route::post('error-logs/{errorLog}/reopen', [ClientErrorReportController::class, 'reopen'])->whereNumber('errorLog');
    });

    Route::get('/user', function (Request $request) {
        return $request->user();
    })->middleware('auth:sanctum');

    // Dashboard
    Route::get('dashboard/stats', [DashboardController::class, 'stats']);
    Route::get('dashboard/analytics', [AnalyticsController::class, 'stats']);
    Route::post('presence/heartbeat', [UserPresenceController::class, 'heartbeat']);
    Route::get('presence/online', [UserPresenceController::class, 'index']);

    // Global Search
    Route::get('search', [\App\Http\Controllers\SearchController::class, 'index']);
    Route::post('search/ai-summary', [\App\Http\Controllers\SearchAiSummaryController::class, 'stream'])
        ->middleware('throttle:30,1');

    // API Resources
    Route::get('spam-units/stats', [SpamUnitController::class, 'stats']);
    Route::get('spam-units/integration/output-options', [SpamUnitController::class, 'integrationOutputOptions']);
    Route::get('spam-units/integration', [SpamUnitController::class, 'integration']);
    Route::get('spam-units/integration/desa/{desaId}', [SpamUnitController::class, 'integrationByDesa']);
    Route::get('spam-units/air-minum-pekerjaan', [SpamUnitController::class, 'airMinumPekerjaan']);
    Route::post('spam-units/{unitSpam}/pekerjaan', [SpamUnitController::class, 'attachPekerjaan']);
    Route::delete('spam-units/{unitSpam}/pekerjaan/{pekerjaanId}', [SpamUnitController::class, 'detachPekerjaan']);
    Route::post('spam-units/{unitSpam}/sync-pekerjaan', [SpamUnitController::class, 'syncPekerjaan']);
    Route::post('spam-units/{unitSpam}/achievements', [SpamUnitController::class, 'addAchievement']);
    Route::post('spam-units/{unitSpam}/budgets', [SpamUnitController::class, 'addBudget']);
    Route::delete('spam-units/{unitSpam}/budgets/{budgetId}', [SpamUnitController::class, 'deleteBudget']);
    Route::post('spam-units/import', [SpamUnitController::class, 'import']);
    Route::apiResource('spam-units', SpamUnitController::class);
    Route::get('spm-sanitasi/stats', [SpmSanitasiController::class, 'stats']);
    Route::get('spm-sanitasi/capaian', [SpmSanitasiController::class, 'capaian']);
    Route::get('spm-sanitasi/integration', [SpmSanitasiController::class, 'integration']);
    Route::get('spm-sanitasi/integration/desa/{desaId}', [SpmSanitasiController::class, 'integrationByDesa']);
    Route::get('spm-sanitasi/mck-pekerjaan', [SpmSanitasiController::class, 'mckPekerjaan']);
    Route::post('spm-sanitasi/{spmSanitasi}/pekerjaan', [SpmSanitasiController::class, 'attachPekerjaan']);
    Route::delete('spm-sanitasi/{spmSanitasi}/pekerjaan/{pekerjaanId}', [SpmSanitasiController::class, 'detachPekerjaan']);
    Route::get('spm-sanitasi/export', [SpmSanitasiController::class, 'export']);
    Route::get('spm-sanitasi/import/template', [SpmSanitasiController::class, 'downloadTemplate']);
    Route::post('spm-sanitasi/import', [SpmSanitasiController::class, 'import']);
    Route::apiResource('spm-sanitasi', SpmSanitasiController::class);
    Route::apiResource('kecamatan', KecamatanController::class);
    Route::apiResource('desa', DesaController::class);
    Route::apiResource('penyedia', PenyediaController::class)->parameters(['penyedia' => 'penyedia']);
    Route::apiResource('kegiatan', KegiatanController::class);
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
    Route::post('koordinat/validate', [KoordinatValidationController::class, 'validateKoordinat']);
    Route::apiResource('foto', FotoController::class);
    Route::apiResource('users', UserController::class);
    Route::apiResource('roles', RoleController::class);
    Route::apiResource('permissions', PermissionController::class);
    Route::apiResource('permissions', PermissionController::class);
    Route::post('route-permissions/check-access', [RoutePermissionController::class, 'check']);
    Route::get('route-permissions/rules', [RoutePermissionController::class, 'rules']);
    Route::get('route-permissions/user/accessible', [RoutePermissionController::class, 'accessible']);
    Route::post('route-permissions/sync', [RoutePermissionController::class, 'sync'])->middleware('role:admin');
    Route::apiResource('route-permissions', RoutePermissionController::class);
    Route::apiResource('menu-permissions', MenuPermissionController::class);
    Route::post('blog/{blog}/comments', [BlogCommentController::class, 'store'])
        ->middleware('throttle:blog-comments');
    Route::put('blog/comments/{comment}', [BlogCommentController::class, 'update'])
        ->middleware('throttle:blog-comments');
    Route::delete('blog/comments/{comment}', [BlogCommentController::class, 'destroy']);
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

    // Post Pekerjaan Checklist (pekerjaan berkontrak)
    Route::get('post-pekerjaan-checklist', [PostPekerjaanChecklistController::class, 'index']);

    // Custom routes
    Route::get('desa/kecamatan/{kecamatanId}', [DesaController::class, 'byKecamatan']);
    Route::get('kegiatan/tahun/{tahun}', [KegiatanController::class, 'byTahun']);

    // Output
    Route::get('output/summary', [OutputController::class, 'summary']);
    Route::apiResource('output', OutputController::class);

    // Kanban
    Route::get('kanban/board', [KanbanController::class, 'board']);
    Route::post('kanban/cards', [KanbanController::class, 'storeCard']);
    Route::post('kanban/cards/from-tiket', [KanbanController::class, 'importFromTiket']);
    Route::put('kanban/cards/{card}', [KanbanController::class, 'updateCard']);
    Route::patch('kanban/cards/{card}/move', [KanbanController::class, 'moveCard']);
    Route::delete('kanban/cards/{card}', [KanbanController::class, 'destroyCard']);

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
    Route::get('pekerjaan/{pekerjaanId}/progress-estimasi', [PekerjaanProgressEstimasiController::class, 'show']);
    Route::put('pekerjaan/{pekerjaanId}/progress-estimasi', [PekerjaanProgressEstimasiController::class, 'update']);
    Route::get('puspen/progress-fisik', [PuspenProgressFisikController::class, 'index']);
    Route::post('puspen/progress-fisik/bulk-update', [PuspenProgressFisikController::class, 'bulkUpdate']);
    Route::get('puspen/pengawas-kpi', [PuspenPengawasKpiController::class, 'index']);
    Route::get('puspen/pengawas-kpi/{user}', [PuspenPengawasKpiController::class, 'show']);
    Route::get('puspen/pekerjaan/{pekerjaan}/review-notes', [PuspenReviewNoteController::class, 'index']);
    Route::post('puspen/pekerjaan/{pekerjaan}/review-notes', [PuspenReviewNoteController::class, 'store']);
    Route::delete('puspen/review-notes/{puspenReviewNote}', [PuspenReviewNoteController::class, 'destroy']);
    Route::get('puspen/media-library', [PuspenMediaShareController::class, 'mediaLibrary']);
    Route::apiResource('puspen/media-shares', PuspenMediaShareController::class)
        ->parameters(['media-shares' => 'puspenMediaShare'])
        ->only(['index', 'store', 'update', 'destroy']);

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

    // Tools PDFs
    Route::get('tool-pdfs/{toolPdf}/download', [ToolPdfController::class, 'download']);
    Route::post('tool-pdfs/bulk-download', [ToolPdfController::class, 'bulkDownload']);
    Route::post('tool-pdfs/sign', [ToolPdfController::class, 'sign']);
    Route::apiResource('tool-pdfs', ToolPdfController::class)->only(['index', 'store', 'destroy']);

    // Signature libraries
    Route::get('signature-libraries', [SignatureLibraryController::class, 'index']);
    Route::post('signature-libraries', [SignatureLibraryController::class, 'store']);
    Route::delete('signature-libraries/{id}', [SignatureLibraryController::class, 'destroy']);

    // ONLYOFFICE editor config (authenticated)
    Route::get('onlyoffice/media/{media}/config', [OnlyOfficeController::class, 'config']);

    // Chat AI (with sessions, cache, and learning)
    Route::post('chat', [\App\Http\Controllers\ChatController::class, 'chat']);
    Route::post('chat/stream', [\App\Http\Controllers\ChatController::class, 'chatStream']);
    Route::get('chat/sessions', [\App\Http\Controllers\ChatController::class, 'sessions']);
    Route::post('chat/sessions', [\App\Http\Controllers\ChatController::class, 'createSession']);
    Route::delete('chat/sessions/{id}', [\App\Http\Controllers\ChatController::class, 'deleteSession']);
    Route::get('chat/sessions/{id}/messages', [\App\Http\Controllers\ChatController::class, 'sessionMessages']);

});
