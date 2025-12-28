# Changelog

All notable changes to the APIAMIS project will be documented in this file.

## [2025-12-28]

### Added
- **Audit Logging System**: Implemented `Auditable` trait to automatically capture `created`, `updated`, and `deleted` events across core models.
- **Geo-Fencing Validation**: Added backend support for storing and retrieving photo coordinate validation results.
- **Enhanced Photo API**: Updated `FotoController` to process and persist `validasi_koordinat` and `validasi_koordinat_message` metadata.
- **User Profile Fields**: Added `nip` and `jabatan` to `User` model for enhanced reporting and identification.

## [2025-12-26]

### Added
- **Excel Import for Pekerjaan**: Integrated `maatwebsite/excel` to support importing pekerjaan data from Excel files.
  - Implemented `PekerjaanImport` with `WithMultipleSheets` to handle template and reference sheets.
  - Added support for resolving `Kecamatan`, `Desa`, and `Kegiatan` by name during import.
  - Added validation failure reporting to return specific row-level errors to the frontend.
- **Excel Template Export**: Added functionality to download a pre-filled Excel template for pekerjaan imports.
  - Dynamic reference sheets for `Kecamatan`, `Desa`, and `Kegiatan`.
- **User Authentication & OAuth**: Implemented robust authentication system including Google OAuth integration and Laravel Sanctum support.
- **Route-Based Access Control (RBAC)**: Added `CheckRoutePermission` middleware to handle granular permissions for API routes.
- **Calendar API**: Implemented backend support for the calendar feature.
  - Created `Event` model and database migrations (`tbl_events`).
  - Implemented `EventController` with standard RESTful CRUD actions.
  - Added support for event categories, location, and description.
- **New API Endpoints**:
  - `POST /api/pekerjaan/import` - Import pekerjaan from Excel.
  - `GET /api/pekerjaan/import/template` - Download import template.
  - `GET /api/auth/google` - Redirect to Google OAuth.
  - `GET /api/auth/google/callback` - Handle Google OAuth callback.
  - `GET/POST/PUT/DELETE /api/events` - Calendar event management.

## [2025-12-25]

### Added
- **Permission Middleware**: Introduced `CheckRoutePermission` for managing route-level access based on user roles and permissions.
- **Sanctum Configuration**: Added default configuration for Laravel Sanctum to support SPA authentication.

### Fixed
- **Trim Validation**: Added `prepareForValidation` in `PekerjaanImport` to trim whitespace from Excel inputs.
- **Empty Row Handling**: Implemented `SkipsEmptyRows` to ignore trailing empty rows in Excel files.
