# Goal (incl. success criteria):
Enabling Non-Admin Pekerjaan Access. Success is when non-admin users (Pengawas/Pendamping) can see and manage their assigned projects with a premium UI.

# Constraints/Assumptions:
- Users have NIP in the `users` table.
- `pengawas` and `pendamping` tables use NIP for correlation.
- Manual assignments are stored in `user_pekerjaan` table.

# Key decisions:
- Use `scopeByUserRole` in `Pekerjaan` model to centralize authorization.
- Use `whereIn` instead of `whereExists` for subqueries in the scope for better performance and readability.
- Standardize non-admin UI to follow a "Premium Bento Grid" aesthetic.

# State:
  - Done: 
    - Debugged and fixed SQL error in `Pekerjaan::scopeByUserRole` (table name mismatch).
    - Verified assigned data appearing for non-admins (Count: 2 for test user).
    - Redesigned `pekerjaan.index.tsx` for non-admins to match dashboard aesthetic.
    - Fixed `.gitignore` in `storage/` and `storage/app/` to allow tracking of `.docx` templates.
    - Staged `SPK_Template.docx`, `bap_template.docx`, and `ringkasan_kontrak_template.docx` for commit.
    - Filtered `Register Dokumen` to only show work packages with existing contracts (`has('kontrak')`).
    - Consolidated `Register Dokumen` frontend to a single table view, removing redundant tabs.
    - Replaced "Register Dinamis" and "Progress BA" with dynamic columns for each `DocumentType`.
    - Removed text truncation and implemented responsive horizontal scrolling with sticky action columns.
    - Fixed `Dockerfile` deployment failure by adding `python3`, `make`, and `g++` to the `asset-builder` stage.
    - Optimized build performance with BuildKit Cache Mounts.
    - Fixed LibreOffice PDF export in Docker by setting `HOME=/tmp` and using custom user profile directory.
    - Updated README.md and CHANGELOG.md with recent features.
  - Now:
    - Verifying system stability after deployment.
  - Next:
    - User to re-attempt deployment.

# Open questions (UNCONFIRMED if needed):
- None.

# Working set (files/ids/commands):
- App\Models\Pekerjaan.php
- c:\laragon\www\arumanis\src\routes\pekerjaan.index.tsx
- c:\laragon\www\arumanis\src\routes\user.dashboard.tsx
