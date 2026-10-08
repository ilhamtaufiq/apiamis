//! Rate limiter jendela geser per kunci, setara `throttle:login` (5 per menit per IP dan per email).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Limiter {
    hits: Mutex<HashMap<String, Vec<Instant>>>,
}

impl Limiter {
    /// Mencatat satu percobaan. `Err(detik)` berarti sudah melewati batas.
    pub fn hit(&self, key: &str, max: usize, window: Duration) -> Result<(), u64> {
        self.hit_at(key, max, window, Instant::now())
    }

    fn hit_at(&self, key: &str, max: usize, window: Duration, now: Instant) -> Result<(), u64> {
        let mut map = self.hits.lock().unwrap_or_else(|e| e.into_inner());
        let entries = map.entry(key.to_string()).or_default();
        entries.retain(|t| now.duration_since(*t) < window);
        if entries.len() >= max {
            let oldest = entries[0];
            let wait = window.saturating_sub(now.duration_since(oldest));
            return Err(wait.as_secs().max(1));
        }
        entries.push(now);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_up_to_max_then_blocks_until_window_passes() {
        let l = Limiter::default();
        let w = Duration::from_secs(60);
        let t0 = Instant::now();
        for _ in 0..5 {
            assert!(l.hit_at("ip-a", 5, w, t0).is_ok());
        }
        let retry = l.hit_at("ip-a", 5, w, t0).unwrap_err();
        assert!((1..=60).contains(&retry));
        assert!(l.hit_at("ip-a", 5, w, t0 + Duration::from_secs(61)).is_ok());
    }

    #[test]
    fn keys_are_independent() {
        let l = Limiter::default();
        let w = Duration::from_secs(60);
        let t0 = Instant::now();
        for _ in 0..5 {
            l.hit_at("a", 5, w, t0).unwrap();
        }
        assert!(l.hit_at("b", 5, w, t0).is_ok());
    }
}
