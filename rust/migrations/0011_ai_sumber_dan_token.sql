-- Sumber data jawaban disimpan di riwayat chat, agar tetap tampil setelah halaman dibuka ulang.
ALTER TABLE ai_chat_messages ADD COLUMN sumber TEXT NULL;

-- Pemakaian token per pemanggilan AI, untuk memantau biaya. Diisi bila layanan AI mengembalikan `usage`.
ALTER TABLE ai_logs
  ADD COLUMN prompt_tokens INT UNSIGNED NULL,
  ADD COLUMN completion_tokens INT UNSIGNED NULL;
