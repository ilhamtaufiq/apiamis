-- Penanda kanal pesan chat AI: 'web' atau 'whatsapp'. Riwayat WhatsApp dibaca terpisah dari chat web.
ALTER TABLE ai_chat_messages ADD COLUMN kanal VARCHAR(16) NOT NULL DEFAULT 'web' AFTER kind;
