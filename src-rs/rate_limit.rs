use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, Instant},
};

use parking_lot::Mutex;

#[derive(Debug)]
pub struct InMemoryRateLimiter {
    window: Duration,
    max_requests: usize,
    buckets: Mutex<HashMap<String, VecDeque<Instant>>>,
}

impl InMemoryRateLimiter {
    pub fn new(window: Duration, max_requests: usize) -> Self {
        Self {
            window,
            max_requests,
            buckets: Mutex::new(HashMap::new()),
        }
    }

    pub fn check_and_count(&self, key: &str) -> bool {
        let now = Instant::now();
        let cutoff = now.checked_sub(self.window).unwrap_or(now);

        let mut buckets = self.buckets.lock();
        let bucket = buckets.entry(key.to_string()).or_default();

        while let Some(front) = bucket.front().copied() {
            if front < cutoff {
                bucket.pop_front();
            } else {
                break;
            }
        }

        if bucket.len() >= self.max_requests {
            return false;
        }

        bucket.push_back(now);
        true
    }
}
