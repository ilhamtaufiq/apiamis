# Third-Party Notices

APIAMIS dibangun di atas library open source pihak ketiga. Hak cipta setiap library dimiliki pengembangnya masing-masing, dan lisensi tiap library berlaku untuk penggunaannya.

Dokumen ini mencantumkan dependensi **langsung** crate `api`, `auth`, dan `shared` beserta versi dan lisensi yang tercatat di crates.io. Dibuat pada 2026-10-29. Dependensi transitif tidak dicantumkan satu per satu, tetapi lisensinya bisa dilihat dengan `cargo metadata` atau `cargo tree`.

Image Docker memuat binary hasil build, jadi lisensi yang mensyaratkan pencantuman pemberitahuan (MIT, Apache-2.0, BSD) tetap terpenuhi lewat berkas ini.

## Perlu ditinjau

_Tidak ada._

## Daftar dependensi langsung

| Paket | Versi | Lisensi | Sumber |
|---|---|---|---|
| aes | 0.8.4 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-ciphers |
| axum | 0.8.9 | MIT | https://github.com/tokio-rs/axum |
| base64 | 0.23.1 | MIT OR Apache-2.0 | https://github.com/marshallpierce/rust-base64 |
| bcrypt | 0.17.1 | MIT | https://github.com/Keats/rust-bcrypt |
| calamine | 0.36.1 | MIT | https://github.com/tafia/calamine |
| cbc | 0.1.2 | MIT OR Apache-2.0 | https://github.com/RustCrypto/block-modes |
| chrono | 0.4.45 | MIT OR Apache-2.0 | https://github.com/chronotope/chrono |
| csv | 1.4.0 | Unlicense/MIT | https://github.com/BurntSushi/rust-csv |
| dotenvy | 0.15.7 | MIT | https://github.com/allan2/dotenvy |
| futures-util | 0.3.34 | MIT OR Apache-2.0 | https://github.com/rust-lang/futures-rs |
| hex | 0.4.3 | MIT OR Apache-2.0 | https://github.com/KokaKiwi/rust-hex |
| hmac | 0.12.1 | MIT OR Apache-2.0 | https://github.com/RustCrypto/MACs |
| image | 0.25.10 | MIT OR Apache-2.0 | https://github.com/image-rs/image |
| lettre | 0.11.23 | MIT | https://github.com/lettre/lettre |
| md-5 | 0.10.6 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| pulldown-cmark | 0.13.4 | MIT | https://github.com/raphlinus/pulldown-cmark |
| quick-xml | 0.41.0 | MIT | https://github.com/tafia/quick-xml |
| rand | 0.8.8 | MIT OR Apache-2.0 | https://github.com/rust-random/rand |
| regex | 1.13.1 | MIT OR Apache-2.0 | https://github.com/rust-lang/regex |
| reqwest | 0.12.28 | MIT OR Apache-2.0 | https://github.com/seanmonstar/reqwest |
| rust_xlsxwriter | 0.99.1 | MIT OR Apache-2.0 | https://github.com/jmcnamara/rust_xlsxwriter |
| serde | 1.0.229 | MIT OR Apache-2.0 | https://github.com/serde-rs/serde |
| serde_json | 1.0.151 | MIT OR Apache-2.0 | https://github.com/serde-rs/json |
| sha2 | 0.10.9 | MIT OR Apache-2.0 | https://github.com/RustCrypto/hashes |
| sqlx | 0.8.6 | MIT OR Apache-2.0 | https://github.com/launchbadge/sqlx |
| subtle | 2.6.1 | BSD-3-Clause | https://github.com/dalek-cryptography/subtle |
| tantivy | 0.22.1 | MIT | https://github.com/quickwit-oss/tantivy |
| thiserror | 2.0.21 | MIT OR Apache-2.0 | https://github.com/dtolnay/thiserror |
| tokio | 1.53.2 | MIT | https://github.com/tokio-rs/tokio |
| tower | 0.5.3 | MIT | https://github.com/tower-rs/tower |
| tower-http | 0.6.11 | MIT | https://github.com/tower-rs/tower-http |
| tracing | 0.1.44 | MIT | https://github.com/tokio-rs/tracing |
| tracing-subscriber | 0.3.23 | MIT | https://github.com/tokio-rs/tracing |
| zip | 8.6.0 | MIT | https://github.com/zip-rs/zip2 |
