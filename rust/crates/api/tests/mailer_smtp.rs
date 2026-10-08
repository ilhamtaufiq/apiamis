//! Pengiriman email lewat server SMTP tiruan di TCP: alamat, subject, dan isi multipart teks + HTML.
//!
//! Tidak butuh database. Server tiruan hanya menerima satu pesan dan merekam percakapannya.

use std::sync::{Arc, Mutex};

use api::mailer::{self, SmtpSettings};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

#[derive(Default, Clone)]
struct Recorded {
    lines: Arc<Mutex<Vec<String>>>,
    data: Arc<Mutex<String>>,
}

/// SMTP minimal: EHLO, AUTH PLAIN, MAIL FROM, RCPT TO, DATA, QUIT.
async fn fake_smtp(listener: TcpListener, rec: Recorded) {
    let (socket, _) = listener.accept().await.unwrap();
    let (read, mut write) = socket.into_split();
    let mut reader = BufReader::new(read);
    write.write_all(b"220 fake.local ESMTP\r\n").await.unwrap();
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.unwrap() == 0 {
            break;
        }
        let cmd = line.trim_end().to_string();
        let upper = cmd.to_uppercase();
        if upper.starts_with("EHLO") || upper.starts_with("HELO") {
            write
                .write_all(b"250-fake.local\r\n250-AUTH PLAIN LOGIN\r\n250 8BITMIME\r\n")
                .await
                .unwrap();
        } else if upper.starts_with("AUTH") {
            rec.lines.lock().unwrap().push("AUTH".into());
            write
                .write_all(b"235 2.7.0 Authentication successful\r\n")
                .await
                .unwrap();
        } else if upper.starts_with("MAIL FROM") || upper.starts_with("RCPT TO") {
            rec.lines.lock().unwrap().push(cmd.clone());
            write.write_all(b"250 OK\r\n").await.unwrap();
        } else if upper == "DATA" {
            write
                .write_all(b"354 End data with <CR><LF>.<CR><LF>\r\n")
                .await
                .unwrap();
            let mut data = String::new();
            loop {
                line.clear();
                reader.read_line(&mut line).await.unwrap();
                if line == ".\r\n" {
                    break;
                }
                data.push_str(&line);
            }
            *rec.data.lock().unwrap() = data;
            write.write_all(b"250 OK queued\r\n").await.unwrap();
        } else if upper == "QUIT" {
            write.write_all(b"221 Bye\r\n").await.unwrap();
            break;
        } else {
            write.write_all(b"502 Unrecognized\r\n").await.unwrap();
        }
    }
}

#[tokio::test]
async fn sends_multipart_text_and_html_through_smtp() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let rec = Recorded::default();
    tokio::spawn(fake_smtp(listener, rec.clone()));

    let settings = SmtpSettings {
        host: "127.0.0.1".into(),
        port,
        // Bukan ssl/tls: opportunistic, server tiruan tidak menawarkan STARTTLS.
        encryption: String::new(),
        username: "pengirim@example.test".into(),
        password: "rahasia".into(),
        from_address: "pengirim@example.test".into(),
        from_name: "Arumanis Uji".into(),
    };
    mailer::send(
        &settings,
        "Pengawas@Example.Test",
        Some("Pengawas Uji"),
        "Instruksi: Lengkapi Data Addendum Kontrak",
        "Yth. Pengawas Uji,\n\nMohon dilengkapi data addendum.",
        Some("<p>Yth. Pengawas Uji, mohon dilengkapi data addendum.</p>"),
    )
    .await
    .expect("email terkirim");

    let lines = rec.lines.lock().unwrap().clone();
    assert!(lines.iter().any(|l| l == "AUTH"), "login SMTP: {lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("MAIL FROM:<pengirim@example.test>")),
        "{lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("RCPT TO:<pengawas@example.test>")),
        "alamat penerima dikecilkan: {lines:?}"
    );

    let data = rec.data.lock().unwrap().clone();
    assert!(
        data.contains("Subject: Instruksi: Lengkapi Data Addendum Kontrak"),
        "{data}"
    );
    assert!(
        data.contains("Content-Type: multipart/alternative"),
        "{data}"
    );
    assert!(
        data.contains("text/plain") && data.contains("text/html"),
        "{data}"
    );
    assert!(data.contains("Mohon dilengkapi data addendum"), "{data}");
    assert!(data.contains("<p>Yth. Pengawas Uji"), "{data}");
    assert!(
        data.contains("\"Arumanis Uji\" <pengirim@example.test>"),
        "{data}"
    );
}

#[tokio::test]
async fn sends_plain_text_when_no_html() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let rec = Recorded::default();
    tokio::spawn(fake_smtp(listener, rec.clone()));

    let settings = SmtpSettings {
        host: "127.0.0.1".into(),
        port,
        encryption: String::new(),
        username: "pengirim@example.test".into(),
        password: "rahasia".into(),
        from_address: "pengirim@example.test".into(),
        from_name: "Arumanis".into(),
    };
    mailer::send(
        &settings,
        "penerima@example.test",
        None,
        "Uji teks",
        "Hanya teks",
        None,
    )
    .await
    .expect("email teks terkirim");
    let data = rec.data.lock().unwrap().clone();
    assert!(data.contains("Subject: Uji teks"), "{data}");
    assert!(!data.contains("multipart/alternative"), "{data}");
    assert!(data.contains("Hanya teks"), "{data}");
}

/// Pengaturan SMTP dari `app_settings`: nonaktif, default, dan kolom wajib yang kosong.
/// Butuh `DATABASE_URL`.
#[tokio::test]
#[ignore = "butuh DATABASE_URL"]
async fn load_settings_follows_app_settings_rules() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL belum di-set");
    let pool = sqlx::MySqlPool::connect(&url).await.unwrap();
    let keys = [
        "mail_enabled",
        "mail_host",
        "mail_port",
        "mail_encryption",
        "mail_username",
        "mail_password",
        "mail_from_address",
        "mail_from_name",
    ];
    let reset = |pool: sqlx::MySqlPool| async move {
        for k in keys {
            sqlx::query("DELETE FROM app_settings WHERE `key` = ?")
                .bind(k)
                .execute(&pool)
                .await
                .unwrap();
        }
    };
    let set = |pool: sqlx::MySqlPool, key: &'static str, value: &'static str| async move {
        sqlx::query("INSERT INTO app_settings (`key`, `value`, `type`, created_at, updated_at) VALUES (?, ?, 'text', NOW(), NOW())")
            .bind(key)
            .bind(value)
            .execute(&pool)
            .await
            .unwrap();
    };

    reset(pool.clone()).await;
    // Nonaktif: tidak ada pengiriman.
    set(pool.clone(), "mail_enabled", "0").await;
    assert_eq!(mailer::load_settings(&pool).await.unwrap(), None);

    // Aktif tapi password kosong: juga tidak ada pengiriman.
    reset(pool.clone()).await;
    set(pool.clone(), "mail_enabled", "1").await;
    set(pool.clone(), "mail_username", "pengirim@example.test").await;
    assert_eq!(mailer::load_settings(&pool).await.unwrap(), None);

    // Aktif dengan kredensial: default port 587, encryption tls, dan dari = username.
    set(pool.clone(), "mail_password", "rahasia").await;
    let s = mailer::load_settings(&pool)
        .await
        .unwrap()
        .expect("pengaturan lengkap");
    assert_eq!(s.host, "smtp.gmail.com");
    assert_eq!(s.port, 587);
    assert_eq!(s.encryption, "tls");
    assert_eq!(s.from_address, "pengirim@example.test");
    assert_eq!(s.from_name, "Arumanis");

    // Laravel memberi default 587 pada `mail_port`, jadi `ssl` tetap memakai 587 kecuali port bernilai 0.
    set(pool.clone(), "mail_encryption", "ssl").await;
    let s = mailer::load_settings(&pool).await.unwrap().unwrap();
    assert_eq!(s.port, 587);
    sqlx::query("UPDATE app_settings SET value = '0' WHERE `key` = 'mail_port'")
        .execute(&pool)
        .await
        .ok();
    set(pool.clone(), "mail_port", "0").await;
    let s = mailer::load_settings(&pool).await.unwrap().unwrap();
    assert_eq!(s.port, 465);

    reset(pool.clone()).await;
}
