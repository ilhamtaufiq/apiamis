# Continuity Ledger

- Goal: 
    1. Implement automatic sequence generation for "Berita Acara" numbers.
    2. Revert to HTTP for local development (Frontend & Backend).
- Constraints/Assumptions:
    - Counter is global across all document types (BA LPP, STP, PHP, STP) within a year.
    - Local access: `http://localhost:5173` (Frontend) and `http://apiamis.test` (Backend).
- Key decisions:
    - Revert `VITE_API_BASE_URL` to HTTP.
    - Revert `vite.config.ts` to disable basic-ssl and HTTPS.
    - Keep both HTTP/HTTPS in `cors.php` for flexibility.
- State:
  - Done:
    - BA Number generation fully implemented (Model, Service, Controller, UI).
    - Reverted all systems to HTTP mode (local dev).
    - Fixed all IDE warnings and errors in `vite.config.ts`.
    - Cleared Laravel config/route cache.
    - Resolved `model_has_roles` missing table error.
    - Fixed phantom migration issue: Re-ran permission migrations and restored schema.
    - Updated `RoleSeeder` with `admin` and `tfl` roles.
    - Assigned `admin` role to `ilhamtaufiq@gmail.com`.
    - Fixed Role/Permission guard mismatch by standardizing to `web` guard.
  - Now:
    - Verifying role-permission sync functionality.
  - Next:
    - Verify menu visibility based on these roles.
- Open questions (UNCONFIRMED):
    - None.
- Working set:
    - `c:\laragon\www\apiamis\app\Services\BeritaAcaraService.php`
    - `c:\laragon\www\bun\src\features\pekerjaan\components\BeritaAcaraTabContent.tsx`
    - `c:\laragon\www\bun\.env`
    - `c:\laragon\www\bun\vite.config.ts`
