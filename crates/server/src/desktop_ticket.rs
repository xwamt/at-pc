//! Ephemeral ticket store for authenticating remote desktop WebSocket sessions.
//!
//! Controls access to `/api/terminals/:id/desktop/ws` via short-lived, single-use tickets
//! generated after verifying client credentials on the REST control plane.

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use crate::config::Role;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TicketError {
    NotFound,
    Expired,
    TerminalMismatch,
}

impl std::fmt::Display for TicketError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound => write!(f, "ticket not found or already consumed"),
            Self::Expired => write!(f, "ticket has expired"),
            Self::TerminalMismatch => write!(f, "ticket terminal ID mismatch"),
        }
    }
}

impl std::error::Error for TicketError {}

#[derive(Debug, Clone)]
pub struct DesktopTicket {
    pub ticket: String,
    pub terminal_id: String,
    pub role: Role,
    pub expires_at: Instant,
}

#[derive(Debug)]
pub struct DesktopTicketStore {
    tickets: RwLock<HashMap<String, DesktopTicket>>,
    ttl: Duration,
}

impl Default for DesktopTicketStore {
    fn default() -> Self {
        Self::new(Duration::from_secs(30))
    }
}

impl DesktopTicketStore {
    pub fn new(ttl: Duration) -> Self {
        Self {
            tickets: RwLock::new(HashMap::new()),
            ttl,
        }
    }

    /// Issue a new single-use ticket for a terminal and role.
    pub fn issue(&self, terminal_id: &str, role: Role) -> String {
        let mut random_bytes = [0u8; 24];
        getrandom::getrandom(&mut random_bytes).expect("system RNG available");
        let hex: String = random_bytes.iter().map(|b| format!("{b:02x}")).collect();
        let ticket = format!("rdt_{hex}");

        let now = Instant::now();
        let expires_at = now + self.ttl;

        let entry = DesktopTicket {
            ticket: ticket.clone(),
            terminal_id: terminal_id.to_string(),
            role,
            expires_at,
        };

        let mut map = self.tickets.write().unwrap();
        // Prune expired tickets
        map.retain(|_, v| v.expires_at > now);
        map.insert(ticket.clone(), entry);

        ticket
    }

    /// Consume an issued ticket, verifying that it matches `terminal_id` and has not expired.
    /// The ticket is single-use and is immediately removed from the store upon lookup.
    pub fn consume(&self, ticket: &str, terminal_id: &str) -> Result<Role, TicketError> {
        let mut map = self.tickets.write().unwrap();
        let entry = map.remove(ticket).ok_or(TicketError::NotFound)?;

        if Instant::now() > entry.expires_at {
            return Err(TicketError::Expired);
        }

        if entry.terminal_id != terminal_id {
            return Err(TicketError::TerminalMismatch);
        }

        Ok(entry.role)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_issue_and_consume_success() {
        let store = DesktopTicketStore::default();
        let ticket = store.issue("term-123", Role::Admin);
        assert!(ticket.starts_with("rdt_"));

        let role = store.consume(&ticket, "term-123").unwrap();
        assert_eq!(role, Role::Admin);

        // Single-use: consuming again must fail
        assert_eq!(
            store.consume(&ticket, "term-123"),
            Err(TicketError::NotFound)
        );
    }

    #[test]
    fn test_consume_terminal_mismatch() {
        let store = DesktopTicketStore::default();
        let ticket = store.issue("term-123", Role::Admin);

        assert_eq!(
            store.consume(&ticket, "term-999"),
            Err(TicketError::TerminalMismatch)
        );
    }

    #[test]
    fn test_consume_expired() {
        let store = DesktopTicketStore::new(Duration::from_millis(10));
        let ticket = store.issue("term-123", Role::Admin);
        std::thread::sleep(Duration::from_millis(20));

        assert_eq!(
            store.consume(&ticket, "term-123"),
            Err(TicketError::Expired)
        );
    }
}
