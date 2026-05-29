# Rencana Implementasi Addendum Kontrak

Dokumen ini dipakai untuk melanjutkan fitur addendum kontrak secara bertahap tanpa mengulang analisis dari awal.

## Tujuan

- Mencatat perubahan kontrak lebih dari satu kali per pekerjaan.
- Menyimpan histori perubahan nilai, tanggal selesai, dan spesifikasi teknis.
- Menyediakan alur draft, diajukan, disetujui, dan ditolak.
- Menyimpan addendum sebagai versi kontrak tersendiri tanpa memodifikasi kontrak utama.
- Menyediakan jejak audit dan lampiran dokumen addendum.

## Status Saat Ini

### Sudah Ada di Workspace

- Migration tabel addendum kontrak sudah dibuat:
  - `tbl_kontrak_addendums`
  - `tbl_kontrak_addendum_items`
- Model addendum sudah dibuat:
  - `KontrakAddendum`
  - `KontrakAddendumItem`
- Relasi addendum di model `Kontrak` sudah ditambahkan:
  - `Kontrak::addendums()`
  - `Kontrak::approvedAddendums()`
  - `Kontrak::latestApprovedAddendum()`
- Resource response addendum sudah dibuat:
  - `KontrakAddendumResource`
- Controller addendum sudah dibuat:
  - `KontrakAddendumController`
- Route addendum sudah ditambahkan ke `routes/api.php`.
- Detail kontrak sudah memuat data addendum:
  - `addendums`
  - `latest_approved_addendum`
  - `contract_versions`
  - `nilai_kontrak_berjalan`
  - `tgl_selesai_berjalan`
- Frontend API module addendum sudah ditambahkan di fitur kontrak.
- Tipe data addendum frontend sudah ditambahkan:
  - `KontrakAddendum`
  - `KontrakAddendumItem`
  - `KontrakVersion`
  - `KontrakAddendumPayload`
- Panel addendum di detail kontrak sudah dibuat:
  - `KontrakAddendumPanel`
- Tampilan detail kontrak sudah menampilkan daftar versi kontrak utama dan semua addendum.
- Dialog create addendum sudah tersedia untuk admin.
- Submit, approve, reject, dan upload dokumen sudah dihubungkan di UI detail kontrak.

### Belum Selesai

- Request validation form belum dipisah ke class tersendiri.
- Form update addendum di UI belum dibuat.
- Delete addendum di UI belum dibuat.
- Input item/detail addendum di UI belum dibuat.
- Upload file addendum sudah ada di UI, tetapi preview/daftar lampiran belum ditampilkan secara lengkap.
- Integrasi kalender untuk event addendum belum ditambahkan.
- Test backend untuk create/approve/reject belum ada.
- Test frontend untuk tab addendum belum ada.
- Verifikasi end-to-end di browser belum dilakukan.

## Desain Data

### Tabel Utama

`tbl_kontrak_addendums`

- `id`
- `kontrak_id`
- `addendum_ke`
- `nomor_addendum`
- `tanggal_addendum`
- `jenis_addendum`
- `alasan`
- `deskripsi_perubahan`
- `nilai_kontrak_sebelum`
- `nilai_kontrak_sesudah`
- `tgl_selesai_sebelum`
- `tgl_selesai_sesudah`
- `status`
- `created_by`
- `approved_by`
- `approved_at`
- timestamps

### Tabel Detail

`tbl_kontrak_addendum_items`

- `id`
- `addendum_id`
- `nama_item`
- `spesifikasi_sebelum`
- `spesifikasi_sesudah`
- `volume_sebelum`
- `volume_sesudah`
- `harga_sebelum`
- `harga_sesudah`
- `subtotal_sebelum`
- `subtotal_sesudah`
- timestamps

## Alur Bisnis

1. Addendum dibuat untuk kontrak tertentu.
2. Nomor addendum diinput manual terlebih dahulu.
3. Addendum disimpan dalam status `draft`.
4. Addendum bisa diajukan, disetujui, atau ditolak.
5. Addendum dengan status `ditolak` boleh diajukan ulang setelah diperbaiki.
6. Saat addendum disetujui, kontrak utama tidak dimodifikasi.
7. Kontrak utama tetap menjadi baseline awal, sedangkan addendum disimpan sebagai versi perubahan berurutan: addendum ke-1, ke-2, sampai ke-n bila ada.
8. Tampilan kontrak harus menampilkan semua versi: kontrak utama dan seluruh addendum yang terkait.
9. Tampilan nilai/tanggal kontrak berjalan harus dihitung dari addendum disetujui terakhir. Jika belum ada addendum disetujui, gunakan data kontrak utama.
10. Histori perubahan kontrak tidak boleh dihapus. Perubahan harus tetap bisa ditelusuri dari kontrak utama dan tabel addendum.

## Aturan Role dan Permission

- Role `Pengawas` boleh melakukan submit/pengajuan addendum.
- Aksi selain submit hanya boleh dilakukan oleh admin, termasuk create, update, delete, approve, reject, dan upload dokumen addendum.
- Backend tetap menjadi sumber kebenaran authorization. Frontend hanya menyembunyikan/menampilkan aksi sesuai permission.
- Pastikan nama role dicek sesuai data Spatie Permission aktual di database, terutama kapitalisasi `Pengawas`.

## Backend Yang Perlu Diselesaikan

- `KontrakAddendumController` sudah dibuat dengan endpoint:
  - `GET /kontrak/{kontrak}/addendums`
  - `POST /kontrak/{kontrak}/addendums`
  - `GET /kontrak-addendums/{id}`
  - `PUT /kontrak-addendums/{id}`
  - `DELETE /kontrak-addendums/{id}`
  - `POST /kontrak-addendums/{id}/submit`
  - `POST /kontrak-addendums/{id}/approve`
  - `POST /kontrak-addendums/{id}/reject`
  - `POST /kontrak-addendums/{id}/upload`
- Validasi masih inline di controller. Pisahkan ke Form Request bila logic makin besar.
- Update/delete/upload addendum yang sudah disetujui sudah ditolak di backend.
- Addendum dengan status `ditolak` sudah bisa diajukan ulang.
- Approve tidak mengubah `nilai_kontrak` dan `tgl_selesai` pada kontrak utama.
- Helper/resource kontrak berjalan sudah ditambahkan dari addendum disetujui terakhir.
- Nomor addendum divalidasi sebagai input manual, bukan auto-generate.
- Resource/response nested untuk item addendum sudah ditambahkan.
- Tambahkan test backend untuk create, submit, approve, reject, upload, dan larangan edit addendum disetujui.

## Frontend Yang Perlu Diselesaikan

- API client addendum di fitur kontrak sudah dibuat.
- Tipe data `KontrakAddendum`, `KontrakAddendumItem`, dan `KontrakVersion` sudah dibuat.
- Panel addendum sudah ditambahkan di detail kontrak.
- Tampilan daftar versi kontrak yang menampilkan kontrak utama dan semua addendum sudah dibuat.
- Dialog form create addendum sudah dibuat.
- Upload dokumen addendum sudah dihubungkan.
- Badge status draft/diajukan/disetujui/ditolak sudah dibuat.
- Aksi submit, approve, dan reject sudah dibuat sesuai role di UI.
- Tambahkan form update addendum.
- Tambahkan delete addendum di UI.
- Tambahkan input item/detail addendum di UI.
- Tambahkan daftar/preview lampiran addendum.
- Tambahkan test frontend untuk panel addendum.

## Integrasi Lanjutan

- Kalender:
  - event addendum bisa dibuat otomatis saat addendum dibuat atau disetujui.
- Progress:
  - bila addendum mengubah volume atau nilai, target progress perlu diselaraskan secara eksplisit.
- Dokumen:
  - lampiran addendum sebaiknya masuk collection media khusus `kontrak/addendum`.

## Urutan Lanjut

1. Verifikasi end-to-end di browser setelah dev server backend dan frontend aktif.
2. Tambahkan form update addendum.
3. Tambahkan delete addendum di UI.
4. Tambahkan input item/detail addendum.
5. Tambahkan daftar/preview lampiran addendum.
6. Sambungkan kalender dan dokumen bila workflow addendum sudah stabil.
7. Tambahkan test backend dan frontend.

## Verifikasi Terakhir

- `npx tsc --noEmit` di frontend: lolos.
- `vendor/bin/pint --test` untuk file PHP terkait: lolos.
- `php -l` untuk `KontrakAddendumController`: lolos.
- `php artisan route:list --path=kontrak-addendums`: route addendum terdaftar.
- `php artisan test` belum valid untuk fitur ini karena konfigurasi test repo saat ini memakai sqlite in-memory, sementara migration legacy project membutuhkan tabel MySQL yang tidak dibuat lengkap di sqlite test.

## Catatan

- Satu kontrak bisa punya banyak addendum.
- Addendum berikutnya selalu mengikuti keadaan kontrak berjalan terakhir dari addendum disetujui sebelumnya, bukan mengubah baseline awal.
- Kontrak awal tidak dimodifikasi oleh proses approve addendum.
- Tampilan kontrak tidak hanya menampilkan addendum terakhir; semua addendum harus terlihat agar histori versi kontrak lengkap.
