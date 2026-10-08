# Foto dan berkas: penyimpanan dan rencana migrasi

Sumber: `app/Http/Controllers/FotoController.php`, `BerkasController.php`, `app/Models/Foto.php`, `config/filesystems.php`, `config/media-library.php`, `config/queue.php`.

## Penyimpanan (sama untuk foto dan berkas)

- Paket: Spatie Media Library. Disk default `public`, root `storage/app/public`, URL `APP_URL/storage/...`.
- Path (`DefaultPathGenerator`): `storage/app/public/{media_id}/{file_name}`. URL: `APP_URL/storage/{media_id}/{file_name}`. Rust sudah menghitung URL yang sama (lihat `penyedia::dokumen`, `tiket::image_url`).
- Baris `media`: `model_type` (mis. `App\Models\Foto`), `model_id`, `uuid`, `collection_name` (`foto/pekerjaan`, `berkas/dokumen`), `name`, `file_name`, `mime_type`, `disk`, `conversions_disk`, `size`, `manipulations`, `custom_properties`, `generated_conversions`, `responsive_images`, `order_column`, timestamps.
- Batas ukuran: `max_file_size` 50 MB.

## Foto (`/api/foto`)

- Store: validasi `pekerjaan_id`, `komponen_id`, `penerima_id`, `koordinat` (wajib), `file` (jpg/jpeg/png, maks 50 MB). Berkas disimpan ke disk lalu satu baris `media`.
- Konversi `thumb` dibuat sinkron saat upload (`->nonQueued()` di `Foto::registerMediaConversions`), bukan lewat queue. Di Rust thumbnail tidak dibuat (T34). `foto_thumb_url` jatuh ke URL asli, jadi bentuk JSON sama, hanya berkas yang diunduh lebih besar.
- Update dan destroy juga menulis media dan menghapus berkas.

## Berkas (`/api/berkas`)

- `convertToPdf`: konversi Word/PDF. Bergantung keputusan K2 (tooling dokumen). Belum dipindah.
- `download-all-berkas`: membuat ZIP. Berisiko memori untuk banyak berkas. Perlu streaming.
- `upload-from-url`: mengambil berkas dari URL luar. Perlu daftar host yang diizinkan (SSRF).
- `quick-share`: membuat tautan berbagi.

## Rencana

1. Foto CRUD di Rust selesai: daftar, show, store, update, destroy, dan bulk destroy (`foto.rs`, `media.rs`). Thumbnail dibuat (T34).
2. Berkas CRUD (tanpa konversi PDF, ZIP, dan upload-from-URL) di Rust.
3. Konversi PDF, ZIP streaming, dan upload-from-URL setelah K2 diputuskan.

## Keputusan yang dibutuhkan

- K2: tooling Word/PDF (LibreOffice di container, layanan terpisah, atau tetap di Laravel).
- Thumbnail: dibiarkan tidak dibuat, atau worker Laravel tetap berjalan untuk konversi (K4).
