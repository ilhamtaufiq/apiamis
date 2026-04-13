# Continuity Ledger

- Goal: 
    1. Implement automatic sequence generation for "Berita Acara" and "Kontrak" (SPPBJ, SPK, SPMK) numbers.
    2. Revert to HTTP for local development (Frontend & Backend).
    3. Implement Word Mail Merge (DocX Template) for "Berita Acara" and "Kontrak" documents.
    4. Complete OpenAPI (Swagger) documentation for all controllers.
    5. Implement API Key authentication as an alternative to Bearer Token.
    6. Implement AI Chat backend integration with MiniMax.
- Constraints/Assumptions:
    - Swagger uses L5-Swagger (OpenAPI 3.0).
    - API Key header: `X-API-KEY`.
    - Key stored in `APP_API_KEY` in `.env`.
- Key decisions:
    - Defined `apiKeyAuth` security scheme in global OpenAPI configuration.
    - Added `@OA` annotations to almost all controllers.
    - Updated security in relevant endpoints to support both `bearerAuth` and `apiKeyAuth`.
- State:
  - Done:
    - [BA/Kontrak numbering implementation...]
    - [Word Mail Merge implementation...]
    - Added OpenAPI annotations to `ChecklistItemController`, `DataQualityController`, `DraftPekerjaanController`, `EventController`, `KegiatanRoleController`, `MenuPermissionController`, `PekerjaanChecklistController`, `PermissionController`, `RoleController`, `RoutePermissionController`, `TagController`, `TiketCommentController`, `UserPekerjaanController`, `SimulationNetworkController`, `RABAnalyzerController`.
    - Integrated `apiKeyAuth` into global Swagger configuration and specific endpoints.
    - Successfully regenerated API documentation via `php artisan l5-swagger:generate`.
  - Now:
    - Submitting finalized API documentation and security updates.
    - Implementing MiniMaxService and ChatController for AI Chat.
  - Next:
    - Testing AI Chat with live database context.
    - Verification of API Key authentication on live endpoints if requested.
- Open questions (UNCONFIRMED):
    - None.
- Working set:
    - `c:\laragon\www\apiamis\app\Http\Controllers\Controller.php` (Security Scheme)
    - `All Controllers in app/Http/Controllers` (OpenAPI Annotations)

