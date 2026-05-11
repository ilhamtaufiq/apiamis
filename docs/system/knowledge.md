# FITUR PENCARIAN GLOBAL (Search Engine Arumanis)

Sistem Arumanis memiliki mesin pencari global yang diatur oleh `SearchController`. Ami harus mengetahui kapabilitas ini untuk membantu pengguna menemukan data.

## Entitas yang Dapat Dicari:
1.  **Pekerjaan (Paket)**: Mencari berdasarkan Nama Paket atau Kode Rekening.
2.  **Kontrak**: Mencari berdasarkan Nomor SPK, SPMK, Kode Paket, atau Nama Paket terkait.
3.  **Penyedia (Kontraktor)**: Mencari berdasarkan Nama Perusahaan atau Nama Direktur.
4.  **Kegiatan**: Mencari berdasarkan Nama Program, Kegiatan, atau Sub-Kegiatan.
5.  **Wilayah (Desa)**: Mencari berdasarkan Nama Desa.
6.  **Dokumentasi (Foto)**: Mencari berdasarkan Keterangan Foto atau Nama Paket terkait.
7.  **Penerima Manfaat**: Mencari berdasarkan Nama Penerima, NIK, Alamat, atau Nama Paket.
8.  **Output Pekerjaan**: Mencari berdasarkan Komponen (misal: "perpipaan", "bak penampung").
9.  **Log Progress**: Mencari berdasarkan isi laporan progress harian/mingguan.

## Teknologi Pencarian:
- Menggunakan **Full-Text Search (MATCH...AGAINST)** untuk kecepatan dan akurasi tinggi pada kolom teks besar.
- Mendukung filter **Tahun Anggaran** (default ke tahun berjalan).
- Hasil pencarian dikelompokkan berdasarkan tipe entitas untuk memudahkan navigasi.

## Batasan Hasil (Limit):
- Kontrak: Max 15 hasil.
- Pekerjaan & Dokumentasi: Max 10 hasil.
- Penyedia & Desa: Max 5 hasil.

💡 **Tips untuk Ami**: Jika pengguna bertanya "Dimana saya bisa mencari data X?", Ami bisa menyarankan pengguna menggunakan kolom Search di aplikasi karena sistem mendukung pencarian lintas entitas.
