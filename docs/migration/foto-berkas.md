# Foto dan berkas: penyimpanan dan rencana migrasi

Sumber: `app/Http/Controllers/FotoController.php`, `BerkasController.php`, `app/Models/Foto.php`, `config/filesystems.php`, `config/media-library.php`, `config/queue.php`.

## Penyimpanan (sama untuk foto dan berkas)

- Paket: Spatie Media Library. Disk default `public`, root `storage/app/public`, URL `APP_URL/storage/...`.
- Path (`DefaultPathGenerator`): `storage/app/public/{media_id}/{file_name}`. URL: `APP_URL/storage/{media_id}/{file_name}`. Rust sudah menghitung URL yang sama (lihat `penyedia::dokumen`, `tiket::image_url`).
- Baris `media`: `model_type` (mis. `App\Models\Foto`), `model_id`, `uuid`, `collection_name` (`foto/pekerjaan`, `berkas/dokumen`), `name`, `file_name`, `mime_type`, `disk`, `conversions_disk`, `size`, `manipulations`, `custom_properties`, `generated_conversions`, `responsive_images`, `order_column`, timestamps.
- Batas ukuran: `max_file_size` 50 MB.

## Foto (`/api/foto`)

- Store: validasi `pekerjaan_id`, `komponen_id`, `penerima_id`, `koordinat` (wajib), `file` (jpg/jpeg/png, maks 50 MB). Berkas disimpan ke disk lalu satu baris `media`.
- Konversi `thumb` dibuat sinkron saat upload (`->nonQueued()` di `Foto::registerMediaConversions`), bukan lewat queue. Di Rust thumbnail dibuat saat upload (`media::make_thumb`, crop 120×120 dengan `resize_to_fill`, diuji di `foto_db`). Selisih kecil: `sharpen(10)` dari Laravel belum ditiru.
- Update dan destroy juga menulis media dan menghapus berkas.

## Berkas (`/api/berkas`)

- `convertToPdf` (`export-pdf`): konversi Word/PDF. Bergantung keputusan K2 (tooling dokumen). Belum dipindah.
- `download-all-berkas`: sudah di Rust (`pekerjaan_download::download_all_berkas`). ZIP masih disusun di memori (`Cursor<Vec<u8>>`), tidak distream. Laravel memakai ZipStream. Perlu streaming sebelum dipakai untuk pekerjaan dengan banyak berkas besar.
- `upload-from-url`: sudah di Rust (`berkas_upload_url.rs`). Daftar host yang diizinkan perlu dicek ulang.
- `quick-share`: belum dipindah, masih di Laravel.

## Rencana

1. Foto CRUD di Rust selesai: daftar, show, store, update, destroy, dan bulk destroy (`foto.rs`, `media.rs`). Thumbnail dibuat saat upload.
2. Berkas CRUD di Rust selesai, termasuk ZIP dan upload-from-URL. Sisa: `quick-share`.
3. Setelah K2 diputuskan: konversi PDF. ZIP perlu dibuat streaming.

## Keputusan yang dibutuhkan

- K2: tooling Word/PDF (LibreOffice di container, layanan terpisah, atau tetap di Laravel).
- Thumbnail: dibiarkan tidak dibuat, atau worker Laravel tetap berjalan untuk konversi (K4).
