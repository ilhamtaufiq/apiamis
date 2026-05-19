# AGENTS.md

Panduan awal untuk AI agent yang bekerja di repo backend **APIAMIS**.

## Baca dulu

1. `.agent/README.md`
2. `.agent/ARCHITECTURE.md`
3. `.agent/SYSTEM_OVERVIEW.md`
4. `.agent/rules.md`

## Jika tugas menyentuh frontend

Repo pasangan berada di:

```text
C:\laragon\www\bun
```

Untuk perubahan kontrak API, cek pemanggil frontend sebelum mengubah serializer atau bentuk response.

## Workflow yang tersedia

- `.agent/workflows/full-stack-feature.md`
- `.agent/workflows/debug-endpoint.md`
- `.agent/workflows/change-api-contract.md`
- `.agent/workflows/create-feature.md`
- `.agent/workflows/testing.md`

## Prinsip singkat

- Backend adalah sumber kebenaran untuk validasi dan authorization akhir.
- Jangan menganggap semua controller sudah memakai `FormRequest`.
- Jangan ubah shape response tanpa mengecek frontend pasangan.
- Perhatikan domain `Pekerjaan`; aturan visibility datanya kompleks.

