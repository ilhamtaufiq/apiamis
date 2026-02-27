# Continuity Ledger

- Goal: 
    1. Implement automatic sequence generation for "Berita Acara" and "Kontrak" (SPPBJ, SPK, SPMK) numbers.
    2. Revert to HTTP for local development (Frontend & Backend).
- Constraints/Assumptions:
    - Counter is global across all document types within a year.
    - SPPBJ, SPK, SPMK use 3-digit padding and include package sequence.
    - Local access: `http://localhost:5173` (Frontend) and `http://apiamis.test` (Backend).
- Key decisions:
    - Unified document numbering management in `BeritaAcaraTabContent`.
    - Integrated `Kontrak` (SPPBJ, SPK, SPMK) into the same UI tab for better DX.
    - Used `Pekerjaan` count within the year to determine package index.
- State:
  - Done:
    - BA and Kontrak number generation fully implemented (Model, Service, Controller, UI).
    - Fixed SPPBJ, SPK, SPMK format: `602.4/[TYPE]/PPK/DISPERKIM-AMS.[PackageIdx].[Seq]/[Year]`.
    - Added `generateNumber` endpoint to `KontrakController`.
    - Unified UI in `BeritaAcaraTabContent` to manage both BA and Kontrak docs.
    - Fixed build error in `DraftPekerjaanList.tsx` (unused imports).
    - Reverted all systems to HTTP mode (local dev).
    - Fixed Role/Permission guard mismatch and assigned admin role.
  - Now:
    - Verification completed, build is successful.
  - Next:
    - None.
- Open questions (UNCONFIRMED):
    - None.
- Working set:
    - `c:\laragon\www\apiamis\app\Services\BeritaAcaraService.php`
    - `c:\laragon\www\apiamis\app\Http\Controllers\KontrakController.php`
    - `c:\laragon\www\bun\src\features\pekerjaan\components\BeritaAcaraTabContent.tsx`
    - `c:\laragon\www\bun\src\features\pekerjaan\components\DraftPekerjaanList.tsx`

