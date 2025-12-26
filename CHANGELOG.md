# Changelog

All notable changes to the APIAMIS project will be documented in this file.

## [2025-12-26]

### Added
- **Excel Import for Pekerjaan**: Integrated `maatwebsite/excel` to support importing pekerjaan data from Excel files.
  - Implemented `PekerjaanImport` with `WithMultipleSheets` to handle template and reference sheets.
  - Added support for resolving `Kecamatan`, `Desa`, and `Kegiatan` by name during import.
  - Added validation failure reporting to return specific row-level errors to the frontend.
- **Excel Template Export**: Added functionality to download a pre-filled Excel template for pekerjaan imports.
  - Dynamic reference sheets for `Kecamatan`, `Desa`, and `Kegiatan`.
- **New API Endpoints**:
  - `POST /api/pekerjaan/import` - Import pekerjaan from Excel.
  - `GET /api/pekerjaan/import/template` - Download import template.

### Fixed
- **Trim Validation**: Added `prepareForValidation` in `PekerjaanImport` to trim whitespace from Excel inputs.
- **Empty Row Handling**: Implemented `SkipsEmptyRows` to ignore trailing empty rows in Excel files.
