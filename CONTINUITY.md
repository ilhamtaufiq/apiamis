# Continuity Ledger

- Goal: 
    1. Implement automatic sequence generation for "Berita Acara" and "Kontrak" (SPPBJ, SPK, SPMK) numbers.
    2. Revert to HTTP for local development (Frontend & Backend).
    3. Implement Word Mail Merge (DocX Template) for "Berita Acara" and "Kontrak" documents.
- Constraints/Assumptions:
    - Counter is global across all document types within a year.
    - SPPBJ, SPK, SPMK use 3-digit padding and include package sequence.
    - Local access: `http://localhost:5173` (Frontend) and `http://apiamis.test` (Backend).
    - Template file: `c:\laragon\www\apiamis\Template_Kontrak.docx`.
    - Templates will be in `.docx` format with `{{placeholder}}` or `${placeholder}` syntax.
- Key decisions:
    - Unified document numbering management in `BeritaAcaraTabContent`.
    - Integrated `Kontrak` (SPPBJ, SPK, SPMK) into the same UI tab for better DX.
    - Used `Pekerjaan` count within the year to determine package index.
    - Use `PHPOffice/PHPWord` for server-side template processing.
- State:
  - Done:
    - BA and Kontrak number generation fully implemented (Model, Service, Controller, UI).
    - Fixed SPPBJ, SPK, SPMK format: `602.4/[TYPE]/PPK/DISPERKIM-AMS.[PackageIdx].[Seq]/[Year]`.
    - Added `generateNumber` endpoint to `KontrakController`.
    - Unified UI in `BeritaAcaraTabContent` to manage both BA and Kontrak docs.
    - Fixed build error in `DraftPekerjaanList.tsx` (unused imports).
    - Reverted all systems to HTTP mode (local dev).
    - Fixed Role/Permission guard mismatch and assigned admin role.
    - Updated `generateNumber` to preview numbers without saving to DB; counter now increments upon contract save.
    - Added UI to RegisterDokumen to edit and manually update the document sequence `last_number` for each year.
    - Implemented Word Mail Merge feature using `PHPOffice/PHPWord`.
    - Created `DocumentExportService` for template processing.
    - Added "Export Word" button in `BeritaAcaraTabContent` UI.
    - Added "Hapus" (delete) function in `RegisterDokumen.tsx` to delete Pekerjaan and its penomoran directly from the register.
  - Now:
    - Fixed "Undefined variable $saveToDb" in `BeritaAcaraService.php`.
  - Next:
    - Verify with user if the missing variable issue is resolved and everything functions as expected.
- Open questions (UNCONFIRMED):
    - None.
- Working set:
    - `c:\laragon\www\apiamis\app\Services\BeritaAcaraService.php`
    - `c:\laragon\www\apiamis\app\Http\Controllers\KontrakController.php`
    - `c:\laragon\www\bun\src\features\pekerjaan\components\BeritaAcaraTabContent.tsx`
    - `c:\laragon\www\bun\src\features\pekerjaan\components\RegisterDokumen.tsx`

