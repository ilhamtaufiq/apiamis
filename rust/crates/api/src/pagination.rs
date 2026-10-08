//! Bentuk respon paginator Laravel: `data`, `links`, dan `meta`.
//!
//! Aturan daftar nomor halaman mengikuti fixture produksi untuk halaman 1 dari
//! 25 halaman (`fixtures/live/desa_index.json`). Untuk halaman lain, aturan
//! ini belum diverifikasi terhadap Laravel dan perlu dicek ulang.

use serde_json::{json, Value};
use std::collections::HashMap;

pub const DEFAULT_PER_PAGE: u64 = 15;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageParams {
    pub page: u64,
    pub per_page: u64,
}

pub fn page_params(query: &HashMap<String, String>) -> PageParams {
    let page = query
        .get("page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(1);
    let per_page = query
        .get("per_page")
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|p| *p >= 1)
        .unwrap_or(DEFAULT_PER_PAGE);
    PageParams { page, per_page }
}

enum Slot {
    Page(u64),
    Dots,
}

/// Nomor halaman yang ditampilkan di `meta.links`.
fn window(current: u64, last: u64) -> Vec<Slot> {
    use Slot::{Dots, Page};
    if last < 14 {
        return (1..=last).map(Page).collect();
    }
    if current <= 7 {
        let mut v: Vec<Slot> = (1..=10).map(Page).collect();
        v.push(Dots);
        v.extend([Page(last - 1), Page(last)]);
        v
    } else if current + 6 >= last {
        let mut v = vec![Page(1), Page(2), Dots];
        v.extend((last - 9..=last).map(Page));
        v
    } else {
        let mut v = vec![Page(1), Page(2), Dots];
        v.extend((current - 3..=current + 3).map(Page));
        v.extend([Dots, Page(last - 1), Page(last)]);
        v
    }
}

/// Membangun respon paginator dari baris yang sudah dipotong `LIMIT/OFFSET`.
///
/// `base_url` adalah `APP_URL` + path, tanpa query string.
pub fn paginate(data: Vec<Value>, total: u64, params: PageParams, base_url: &str) -> Value {
    let PageParams { page, per_page } = params;
    let last_page = total.div_ceil(per_page).max(1);
    let url = |p: u64| format!("{base_url}?page={p}");
    let (from, to) = if total == 0 {
        (Value::Null, Value::Null)
    } else {
        let from = (page - 1) * per_page + 1;
        let to = (page * per_page).min(total);
        (json!(from), json!(to))
    };

    let mut meta_links = vec![json!({
        "active": false,
        "label": "&laquo; Previous",
        "page": Value::Null,
        "url": Value::Null,
    })];
    for slot in window(page, last_page) {
        meta_links.push(match slot {
            Slot::Page(n) => json!({
                "active": n == page,
                "label": n.to_string(),
                "page": n,
                "url": url(n),
            }),
            Slot::Dots => json!({ "active": false, "label": "...", "url": Value::Null }),
        });
    }
    let next_page = (page < last_page).then_some(page + 1);
    meta_links.push(json!({
        "active": false,
        "label": "Next &raquo;",
        "page": next_page,
        "url": next_page.map(url),
    }));

    json!({
        "data": data,
        "links": {
            "first": url(1),
            "last": url(last_page),
            "prev": (page > 1).then(|| url(page - 1)),
            "next": next_page.map(url),
        },
        "meta": {
            "current_page": page,
            "from": from,
            "last_page": last_page,
            "links": meta_links,
            "path": base_url,
            "per_page": per_page,
            "to": to,
            "total": total,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_matches_fixture_for_first_of_25_pages() {
        let labels: Vec<String> = window(1, 25)
            .into_iter()
            .map(|s| match s {
                Slot::Page(n) => n.to_string(),
                Slot::Dots => "...".to_string(),
            })
            .collect();
        assert_eq!(
            labels,
            vec!["1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "...", "24", "25"]
        );
    }

    #[test]
    fn small_result_sets_list_every_page() {
        let v = paginate(
            vec![],
            20,
            page_params(&HashMap::new()),
            "http://x/api/kegiatan",
        );
        assert_eq!(v["meta"]["last_page"], 2);
        assert_eq!(v["meta"]["links"].as_array().unwrap().len(), 4);
    }
}
