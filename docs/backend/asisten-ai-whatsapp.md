# Asisten ARUMANIS di WhatsApp

Dokumen ini menjelaskan spesifikasi, alur logika, dan fitur Asisten ARUMANIS ketika dipakai lewat WhatsApp. Kanal ini memakai mesin AI dan data yang sama dengan Asisten Data di aplikasi web, dengan perbedaan pada cara pesan masuk dan keluar, serta riwayat percakapannya.

Status: sebagian fitur sudah di `main`, sebagian masih menunggu PR. Lihat bagian [Status implementasi](#status-implementasi).

## Ringkasan

- Pengguna mengirim pesan ke nomor WhatsApp bot, atau menyebut bot di grup.
- open-wa meneruskan pesan ke apiamis sebagai webhook `message.received`.
- apiamis mencocokkan pengirim dengan akun pengguna, lalu menjawab sesuai perintah: `catat`, `buat jadwal`, atau pertanyaan umum yang dijawab AI.
- Jawaban dikirim balik ke chat asal lewat open-wa.

## Komponen

| Komponen | Peran |
|---|---|
| open-wa (`wa-api.cianjur.space`) | Gateway WhatsApp. Menerima dan mengirim pesan, serta mengunduh media. |
| `POST /api/wa/webhook?secret=...` | Titik masuk webhook di apiamis. Memeriksa secret, lalu memproses pesan di latar belakang. |
| `wa_inbound.rs` | Membaca payload, menentukan apakah bot disebut, dan mengarahkan perintah. |
| `wa_gateway.rs` | Klien open-wa: kirim teks, kirim ke chat, unduh media. |
| `ai_memory.rs` | Giliran AI: prompt WhatsApp, tool, dan penyimpanan riwayat. |
| `catatan.rs` | Penyimpanan catatan dan lampiran. |
| `jadwal_wa.rs` | Pembacaan perintah `buat jadwal` menjadi event kalender. |

## Konfigurasi (environment)

| Variabel | Fungsi |
|---|---|
| `WA_API_URL` | Alamat open-wa, misalnya `https://wa-api.cianjur.space`. |
| `WA_API_KEY` | Kunci API open-wa, dikirim sebagai header `X-API-Key`. Tidak pernah dikirim ke klien. |
| `WA_SESSION_ID` | ID sesi WhatsApp yang tersambung. Harus lengkap, termasuk bagian UUID setelah tanda hubung. |
| `WA_WEBHOOK_SECRET` | Secret untuk `?secret=` pada webhook. Tanpa secret, webhook menolak semua permintaan. |
| `WA_BOT_NUMBER` | Nomor bot dalam bentuk `628...`. Dipakai untuk mengenali mention `@nomor` di teks. |
| `WA_BOT_LID` | ID LID bot (`...@lid`). Dipakai untuk mengenali mention di grup. |

Jika `WA_API_URL`, `WA_API_KEY`, atau `WA_SESSION_ID` kosong, pengiriman dinonaktifkan dan endpoint kirim mengembalikan 503.

Di open-wa, webhook dibuat untuk event `message.received` dengan URL `https://apiamis.../api/wa/webhook?secret=...`. Filter kosong berarti semua pesan masuk diteruskan.

## Masuknya pesan

### Format payload

Payload `message.received` dibaca dengan toleransi terhadap beberapa bentuk. Field yang dipakai:

| Keperluan | Field yang dibaca |
|---|---|
| Teks | `body`, `content`, `text` |
| Chat asal | `chatId` (grup berakhiran `@g.us`) |
| Pengirim | `author` untuk grup, `from` untuk chat pribadi. Bisa berupa nomor (`@c.us`) atau LID (`@lid`). |
| Mention | `mentionedIds`, `mentionedJidList`, atau `mentions` |
| Pesan yang dibalas | `quotedMessage` (atau `quotedMsg` bila ada). Dibaca `id`, `body`, `caption`, `fromMe`. |
| Pesan dari bot sendiri | `fromMe: true` diabaikan. |
| Event lain | Selain `message.received` diabaikan. |

Dokumentasi open-wa mendefinisikan `quotedMessage` hanya dengan `id` dan `body`, dan `mentionedIds` sebagai daftar WID yang di-mention. Field lain di `quotedMessage` (tipe, MIME) tidak dijamin ada.

### Apakah bot disebut

Pesan dianggap menyebut bot bila salah satu berlaku:
- `mentionedIds` atau `mentionedJidList` memuat nomor atau LID bot.
- Teks memuat `@<WA_BOT_NUMBER>` atau `@<digit WA_BOT_LID>`.
- Teks memuat `@arumanis`, tanpa membedakan huruf besar dan kecil.
- Pesan yang dibalas berasal dari bot (`fromMe: true`).

Di chat pribadi, bot selalu menjawab. Di grup, bot hanya menjawab bila disebut.

Teks mention (`@arumanis`, `@<nomor>`) dibuang sebelum perintah diproses.

## Alur pemrosesan

Urutan pemeriksaan di `proses`:

1. **Grup tanpa sebutan:** pesan diabaikan dan hanya dicatat di log.
2. **Pengirim dicari:** nomor atau LID pengirim dicocokkan dengan `users.whatsapp`. Di grup, LID diterjemahkan ke nomor lewat open-wa bila memungkinkan.
3. **Belum terdaftar:** bot membalas dengan petunjuk mendaftarkan nomor di menu Profil.
4. **Perintah `catat`:** lihat [Catat](#catat).
5. **Perintah `buat jadwal`:** lihat [Buat jadwal](#buat-jadwal).
6. **Pesan kosong:** bot meminta pertanyaan.
7. **Batas pemakaian:** `cek_batas` membatasi jumlah pertanyaan. Bila terlampaui, bot membalas dengan pesannya.
8. **Peran dan model:** peran pengguna, tahun anggaran aktif, dan model AI dari pengaturan.
9. **Giliran AI:** `ai_memory::giliran` dengan kanal WhatsApp. Lihat [Asisten AI](#asisten-ai).
10. **Balasan:** jawaban dikirim ke chat asal. Bila asisten gagal, bot tetap membalas dengan pesan gagal.

## Fitur

### Tanya data (AI)

- Pertanyaan umum dijawab oleh model AI dengan tool data yang sama seperti di web. Contoh: statistik, kontrak, realisasi keuangan, pekerjaan, catatan paket, usulan penyedia, dan foto atau berkas.
- Hak akses mengikuti peran pengguna. Tool yang memakai `access::restriction` hanya menampilkan pekerjaan yang boleh dilihat pengguna.
- Tool admin tidak ditawarkan kepada pengguna non-admin.
- Jawaban untuk WhatsApp pendek, tanpa tabel markdown, dan memakai format WhatsApp (`*tebal*`, daftar bernomor).
- Sapaan atau tes singkat dibalas satu atau dua kalimat, tanpa mendaftar semua kemampuan.

### Catat

Perintah: membalas sebuah pesan lalu menulis `@arumanis catat`.

- Perintah dikenali bila kalimatnya pendek (maksimal lima kata) dan memuat kata yang diawali `catat`.
- Pesan yang dibalas berupa teks: teksnya disimpan sebagai catatan dengan `sumber = 'whatsapp'` dan `chat_id` asal.
- Pesan yang dibalas berupa media (foto atau berkas): bot mengunduh media lewat `GET /api/sessions/{id}/messages/{chatId}/{messageId}/media`, lalu menyimpannya sebagai lampiran catatan. Keterangan (caption) dipakai sebagai isi; tanpa keterangan, isinya "Lampiran dari WhatsApp".
- Bila unduhan gagal dan ada teks, teksnya yang disimpan. Bila unduhan gagal dan tidak ada teks, bot membalas bahwa lampiran tidak bisa diambil.
- Tanpa balasan sama sekali, bot meminta pengguna membalas pesan yang ingin dicatat.
- Isi maksimal 10.000 karakter.

Catatan hanya terlihat oleh pemiliknya (atau admin dengan `semua=1`) di halaman Catatan arumanis.

### Buat jadwal

Perintah: `buat jadwal` di pesan yang menyebut bot. Judul diambil dari pesan yang dibalas; detail diambil dari teks perintah.

Format yang dikenali (bebas, selama tanggal terbaca):
- Tanggal: `besok`, `lusa`, `hari ini`, `25/10/2026`, `2026-10-25`, atau tanggal dengan nama bulan.
- Waktu: `jam 9`, `pukul 9:30`, `09:00-11:00`. Tanpa waktu, event berlaku seharian.
- Lokasi: kata `tempat`, `lokasi`, atau `di` diikuti nama tempat.

Jika tanggal tidak terbaca, bot membalas dengan contoh format. Jam selesai harus setelah jam mulai. Waktu disimpan sebagai jam dinding WIB di `tbl_events` milik pengirim.

Tanpa jam selesai, durasi default satu jam.

### Lampiran dan keterangan

Lihat [Catat](#catat). Lampiran disimpan lewat jalur media yang sama dengan berkas, dengan model `App\Models\Catatan` dan koleksi `lampiran`. Lampiran belum ditampilkan di halaman Catatan.

### Mention di grup

Bot hanya menjawab di grup bila disebut. Ada tiga cara sebut yang dikenali: `@arumanis`, `@<nomor bot>`, atau mention WhatsApp yang memuat LID bot. Untuk cara ketiga, `WA_BOT_LID` harus diisi.

## Asisten AI

### Riwayat

- Giliran WhatsApp hanya memakai riwayat WhatsApp: 6 pesan terakhir dengan `kanal = 'whatsapp'`. Riwayat chat web tidak dibawa.
- Kanal disimpan di kolom `ai_chat_messages.kanal` (`web` atau `whatsapp`, default `web`).
- Riwayat web masih membaca seluruh pesan, termasuk pesan WhatsApp. Pemisahan hanya satu arah.

### Prompt

Prompt WhatsApp (`PROMPT_WA`) berisi identitas Asisten ARUMANIS, batasan (hanya seputar ARUMANIS, bukan nasihat hukum), dan gaya jawaban pendek. Bagian `{peran}` dan `{tahun}` diisi per giliran.

### Tool

Tool yang tersedia untuk pengguna biasa dan admin ditentukan di `TOOLS` (`ai.rs`). Tool yang dipakai dari WhatsApp sama dengan di web.

### Pemakaian dan log

- Pemakaian tool dan token model dicatat ke `ai_logs` (lihat `ai::catat`).
- Setelah giliran selesai, memori dibangun di latar belakang (`bangun_tertunda`).

## Error dan balasan

| Kondisi | Balasan ke pengguna |
|---|---|
| Nomor belum terdaftar | Petunjuk mendaftarkan nomor di Profil. |
| Pesan kosong | "Silakan tulis pertanyaan Anda." |
| Batas pemakaian tercapai | Pesan dari `cek_batas`. |
| Asisten gagal | "Maaf, Asisten ARUMANIS belum bisa menjawab: ..." |
| Jawaban kosong | "Maaf, Asisten ARUMANIS tidak mendapat jawaban. Coba ulangi pertanyaannya." |
| Gagal mencari pengguna atau membaca peran | "Maaf, Asisten ARUMANIS sedang tidak bisa diakses. Coba lagi beberapa saat." |
| `catat` tanpa balasan | "Balas pesan yang ingin dicatat, lalu tulis @arumanis catat." |
| Lampiran tidak bisa diunduh, tanpa teks | "Lampiran dari pesan yang dibalas tidak bisa diambil dari WhatsApp..." |

Kegagalan teknis dicatat ke log dengan prefiks `[WA]`.

## Diagnostik

Log yang berguna saat pengujian:

- `[WA] diagnostik grup:` untuk pesan grup yang tidak menyebut bot. Mencetak `body`, `mentionedJidList`, dan `mentionedIds`.
- `[WA] catat tanpa kutipan:` mencetak kunci payload dan isi `quoted`.
- `[WA] lampiran tidak bisa diunduh (...)` untuk kegagalan unduh media.
- `[WA] asisten gagal menjawab:` untuk kegagalan giliran AI.

## Keamanan

- Secret webhook dan kunci API open-wa hanya dibaca dari environment. Keduanya tidak boleh ditulis ke kode, log, atau PR.
- Secret yang pernah dibagikan di chat harus diganti, termasuk URL webhook yang memuatnya.
- Webhook menolak permintaan tanpa secret yang benar.
- Akses data mengikuti peran pengguna yang terdaftar, sama seperti di web.

## Batasan saat ini

- Beberapa pengguna WhatsApp bisa diakses semua peran yang terdaftar. Prompt menyebut "hanya untuk tim internal/admin", tetapi kode belum membatasi. Ini perlu diputuskan.
- Lampiran `catat` belum tampil di halaman Catatan.
- Pesan dengan beberapa foto belum ditangani: satu balasan `catat` merujuk satu pesan.
- Bentuk payload untuk foto yang dibalas belum diverifikasi dengan data nyata di grup. Log diagnostik ada untuk memastikannya.
- Daftar tool dan aturan jadwal bisa berubah. Dokumen ini mengikuti kode di `main` dan PR yang disebut di bawah.

## Status implementasi

| Fitur | Status (20 Oktober 2026) |
|---|---|
| Webhook, mention, balasan gagal, kanal riwayat, prompt WhatsApp | PR #66, sudah di-merge |
| Catat teks dan lampiran (dasar) | PR #67, sudah di-merge |
| Unduh lampiran dari setiap pesan yang dibalas | PR #69 |
| Diagnostik mention grup | PR #65 |
| Perbaikan daftar catatan (timestamp) | PR #68 |
| Tool `catatan_pekerjaan` | PR #70 |
| Kolom usulan penyedia (backend dan form) | PR #71 (apiamis), PR #281 (arumanis) |
| Tool `usulan_penyedia` | PR #72 |

Untuk daftar terbaru, lihat PR terkait.
