# APIAMIS (Rust)

Backend APIAMIS, ditulis dengan Axum, SQLx (MySQL), dan Tokio. Penjelasan lengkap ada di [README utama](../README.md).

## Struktur

```
rust/
  Cargo.toml          workspace
  migrations/         SQL schema, dijalankan oleh `apiamis-api migrate`
  crates/
    api/              binary server Axum: router, handler, migrator, fitur
    auth/             login dan sesi
    shared/           konfigurasi dan tipe error API
```

## Menjalankan

```bash
cargo run -p api -- migrate   # migrasi
cargo run -p api              # server, default port 8000 (APP_PORT)
cargo test --workspace
```

Endpoint dasar:

- `GET /up` — health check
- `GET /api/health` — info service
- `GET /api/...` — seluruh API domain

Konfigurasi lengkap ada di [.env.rust.example](../.env.rust.example).
