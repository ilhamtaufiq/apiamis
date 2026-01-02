# Continuity Ledger

- Goal: 
    1. Bypass "tahun anggaran" filter for assigned works (DONE).
    2. Add "Sub Kegiatan" column to Pekerjaan list page (DONE).
    3. Add "Sub Kegiatan" filter to Pekerjaan list page (NOW).
- Constraints/Assumptions:
    - Backend unification in `index()` allows multi-filtering.
- Key decisions:
    - Modify `PekerjaanController@index` to support `kecamatan_id` and `kegiatan_id`.
    - Unify `getPekerjaan` API helper to use `/pekerjaan` with query params.
- State:
  - Done:
    - First two tasks completed.
    - Research for "Sub Kegiatan" filter completed.
    - Implementation plan for filter created.
  - Now:
    - Awaiting user approval for the filter plan.
  - Next:
    - Apply backend and frontend changes.
- Open questions (UNCONFIRMED):
    - None.
- Working set:
    - `c:\laragon\www\apiamis\app\Http\Controllers\PekerjaanController.php`
    - `c:\laragon\www\bun\src\features\pekerjaan\api\pekerjaan.ts`
    - `c:\laragon\www\bun\src\features\pekerjaan\components\PekerjaanList.tsx`
