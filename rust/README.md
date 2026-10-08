# APIAMIS (Rust)

Workspace backend APIAMIS yang sedang dimigrasikan dari Laravel ke Rust (Axum).
Rencana lengkap ada di dokumen migrasi; folder ini masih tahap awal.

## Struktur

```
rust/
  Cargo.toml          workspace
  crates/
    api/              binary server Axum (router, middleware)
    shared/           konfigurasi dan error API
```

Crate lain dari rencana (`db`, `auth`, `storage`, `documents`, `jobs`, `realtime`)
ditambahkan saat modulnya mulai dikerjakan.

## Menjalankan

```bash
cd rust
cargo run -p api          # default port 8000, atau APP_PORT dari .env
cargo test --workspace
```

Endpoint saat ini:

- `GET /up` — health (sama dengan `/up` di Laravel)
- `GET /api/health` — info service
