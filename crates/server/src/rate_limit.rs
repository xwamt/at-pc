//! Rate limiting module for authentication attempts.
//!
//! Protects the at-pc server against brute-force attacks by tracking authentication failures
//! per IP address across rolling time windows:
//! - Rolling 60s window: >= 10 failures returns `RateLimited { retry_after_secs: 60 }` (HTTP 429).
//! - Rolling 10m window: >= 30 failures bans the IP for 15 minutes (900s) (HTTP 429).
//! - Successful authentication resets the failure counter for that IP.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Decision returned by the rate limiter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    Allowed,
    RateLimited { retry_after_secs: u64 },
    Banned { retry_after_secs: u64 },
}

impl RateLimitDecision {
    pub fn is_allowed(&self) -> bool {
        matches!(self, RateLimitDecision::Allowed)
    }

    pub fn retry_after_secs(&self) -> Option<u64> {
        match self {
            RateLimitDecision::Allowed => None,
            RateLimitDecision::RateLimited { retry_after_secs }
            | RateLimitDecision::Banned { retry_after_secs } => Some(*retry_after_secs),
        }
    }
}

#[derive(Debug, Clone)]
struct IpEntry {
    failures: Vec<Instant>,
    banned_until: Option<Instant>,
}

/// Thread-safe in-memory rate limiter for authentication requests.
#[derive(Debug, Default)]
pub struct AuthRateLimiter {
    entries: Mutex<HashMap<IpAddr, IpEntry>>,
}

impl AuthRateLimiter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records an authentication failure for `ip` using `Instant::now()`.
    pub fn record_failure(&self, ip: IpAddr) -> RateLimitDecision {
        self.record_failure_at(ip, Instant::now())
    }

    /// Records an authentication failure for `ip` at a specific `Instant` (useful for testing).
    pub fn record_failure_at(&self, ip: IpAddr, now: Instant) -> RateLimitDecision {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(poisoned) => poisoned.into_inner(),
        };

        let entry = entries.entry(ip).or_insert_with(|| IpEntry {
            failures: Vec::new(),
            banned_until: None,
        });

        // 1. If IP is banned (ban until > now): return Banned
        if let Some(banned_until) = entry.banned_until {
            if banned_until > now {
                let dur = banned_until - now;
                let retry_after = dur.as_secs() + if dur.subsec_nanos() > 0 { 1 } else { 0 };
                return RateLimitDecision::Banned {
                    retry_after_secs: retry_after.max(1),
                };
            } else {
                entry.banned_until = None;
            }
        }

        // Record failure
        entry.failures.push(now);

        // Prune failures older than 10 minutes (600s)
        let ten_mins = Duration::from_secs(600);
        entry.failures.retain(|&t| now.duration_since(t) <= ten_mins);

        // 2. If failures >= 30 in 10 minutes: ban IP for 15 minutes (900s)
        if entry.failures.len() >= 30 {
            entry.banned_until = Some(now + Duration::from_secs(900));
            return RateLimitDecision::Banned {
                retry_after_secs: 900,
            };
        }

        // 3. Count failures in rolling 60-second window
        let one_min = Duration::from_secs(60);
        let failures_60s = entry
            .failures
            .iter()
            .filter(|&&t| now.duration_since(t) <= one_min)
            .count();

        if failures_60s >= 10 {
            return RateLimitDecision::RateLimited {
                retry_after_secs: 60,
            };
        }

        RateLimitDecision::Allowed
    }

    /// Resets the failure counter and any active ban for `ip`.
    pub fn record_success(&self, ip: IpAddr) {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(poisoned) => poisoned.into_inner(),
        };
        entries.remove(&ip);
    }

    /// Checks if `ip` is currently blocked or rate-limited before processing request.
    pub fn check(&self, ip: IpAddr) -> RateLimitDecision {
        self.check_at(ip, Instant::now())
    }

    /// Checks if `ip` is currently blocked or rate-limited at a specific `Instant`.
    pub fn check_at(&self, ip: IpAddr, now: Instant) -> RateLimitDecision {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(poisoned) => poisoned.into_inner(),
        };

        let entry = match entries.get_mut(&ip) {
            Some(e) => e,
            None => return RateLimitDecision::Allowed,
        };

        if let Some(banned_until) = entry.banned_until {
            if banned_until > now {
                let dur = banned_until - now;
                let retry_after = dur.as_secs() + if dur.subsec_nanos() > 0 { 1 } else { 0 };
                return RateLimitDecision::Banned {
                    retry_after_secs: retry_after.max(1),
                };
            } else {
                entry.banned_until = None;
            }
        }

        let one_min = Duration::from_secs(60);
        let failures_60s = entry
            .failures
            .iter()
            .filter(|&&t| now.duration_since(t) <= one_min)
            .count();

        if failures_60s >= 10 {
            return RateLimitDecision::RateLimited {
                retry_after_secs: 60,
            };
        }

        RateLimitDecision::Allowed
    }
}

/// Result of checking request quota for an authenticated key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuotaResult {
    pub allowed: bool,
    pub limit: u32,
    pub remaining: u32,
    pub reset_secs: u64,
}

#[derive(Debug, Clone)]
struct KeyQuotaEntry {
    requests: Vec<Instant>,
}

/// Thread-safe rolling window per-key request quota tracker.
#[derive(Debug)]
pub struct KeyQuotaTracker {
    limit: u32,
    window: Duration,
    entries: Mutex<HashMap<String, KeyQuotaEntry>>,
}

impl Default for KeyQuotaTracker {
    fn default() -> Self {
        Self::new(120, Duration::from_secs(60))
    }
}

impl KeyQuotaTracker {
    pub fn new(limit: u32, window: Duration) -> Self {
        Self {
            limit,
            window,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Checks and records a request for `key_id` at `Instant::now()`.
    pub fn check_and_record(&self, key_id: &str) -> QuotaResult {
        self.check_and_record_at(key_id, Instant::now())
    }

    /// Checks and records a request for `key_id` at a specific `Instant`.
    pub fn check_and_record_at(&self, key_id: &str, now: Instant) -> QuotaResult {
        let mut entries = match self.entries.lock() {
            Ok(e) => e,
            Err(poisoned) => poisoned.into_inner(),
        };

        let entry = entries.entry(key_id.to_string()).or_insert_with(|| KeyQuotaEntry {
            requests: Vec::new(),
        });

        // Prune requests older than rolling window
        entry.requests.retain(|&t| now.duration_since(t) <= self.window);

        let current_count = entry.requests.len() as u32;
        let oldest = entry.requests.first().copied().unwrap_or(now);
        let elapsed = now.duration_since(oldest);
        let reset_secs = if elapsed < self.window {
            (self.window - elapsed).as_secs().max(1)
        } else {
            self.window.as_secs()
        };

        if current_count >= self.limit {
            QuotaResult {
                allowed: false,
                limit: self.limit,
                remaining: 0,
                reset_secs,
            }
        } else {
            entry.requests.push(now);
            let remaining = self.limit.saturating_sub(current_count + 1);
            QuotaResult {
                allowed: true,
                limit: self.limit,
                remaining,
                reset_secs,
            }
        }
    }
}
