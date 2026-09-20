//! In-memory HttpOnly session store for authenticated browser console access.

use crate::keystore::Principal;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct SessionRecord {
    pub id: String,
    pub principal: Principal,
    pub created_at: Instant,
    pub expires_at: Instant,
}

#[derive(Debug)]
pub struct SessionStore {
    ttl: Duration,
    sessions: Mutex<HashMap<String, SessionRecord>>,
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(24 * 3600)) // 24h default TTL
    }
}

impl SessionStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Creates a new session for `principal` using a CSPRNG session token.
    pub fn create_session(&self, principal: Principal) -> String {
        let mut bytes = [0u8; 24];
        getrandom::getrandom(&mut bytes).expect("CSPRNG must not fail");
        let token = format!("atpc_sess_{}", URL_SAFE_NO_PAD.encode(bytes));
        let now = Instant::now();
        let record = SessionRecord {
            id: token.clone(),
            principal,
            created_at: now,
            expires_at: now + self.ttl,
        };

        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(p) => p.into_inner(),
        };
        // Prune expired sessions
        sessions.retain(|_, v| v.expires_at > now);
        sessions.insert(token.clone(), record);
        token
    }

    /// Validates a session token. Returns the associated `Principal` if valid and not expired.
    pub fn validate_session(&self, token: &str) -> Option<Principal> {
        let now = Instant::now();
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(p) => p.into_inner(),
        };
        if let Some(record) = sessions.get(token) {
            if record.expires_at > now {
                return Some(record.principal.clone());
            }
        }
        sessions.remove(token);
        None
    }

    /// Revokes a session token (e.g. on logout).
    pub fn revoke_session(&self, token: &str) {
        let mut sessions = match self.sessions.lock() {
            Ok(s) => s,
            Err(p) => p.into_inner(),
        };
        sessions.remove(token);
    }
}
