//! Identity value objects.
//!
//! These exist so the type checker distinguishes the many string- and
//! integer-shaped things in this domain. `fn snapshot(s: SessionId, t:
//! TurnNumber)` cannot be called with its arguments swapped; `fn snapshot(s:
//! String, t: u32)` can, and eventually will be.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Identifies a session within an agent. Opaque: Codex uses a UUIDv7, Claude
/// Code a UUIDv4, and a future agent may use neither -- the domain does not
/// care, and must not start caring.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct SessionId(String);

impl SessionId {
    /// Rejects blank ids. A value object validates itself at construction, so
    /// no downstream code has to re-check.
    pub fn new(raw: impl Into<String>) -> Result<Self, IdentityError> {
        let raw = raw.into();
        if raw.trim().is_empty() {
            return Err(IdentityError::Blank("SessionId"));
        }
        Ok(Self(raw))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// True when `prefix` matches the start of this id, case-insensitively.
    /// Supports the CLI affordance of typing the first few characters of a
    /// session id rather than the whole UUID.
    pub fn matches_prefix(&self, prefix: &str) -> bool {
        self.0
            .to_ascii_lowercase()
            .starts_with(&prefix.to_ascii_lowercase())
    }

    /// This id rendered safe to name a file with, or `None` if it cannot be.
    ///
    /// The domain does not care what shape an id is, which is exactly why
    /// something has to stand between an id and a path: `..` and a separator
    /// are legal contents of an opaque string and would place a written file
    /// outside the directory that was chosen for it. Every byte outside a
    /// conservative set is percent-encoded rather than rejected, so an odd but
    /// real id from a future agent is still writable rather than refused.
    ///
    /// Here rather than in whichever adapter needed it first because two
    /// separate write paths now name files after ids, and a user reading one
    /// directory beside the other should not find the same session under two
    /// different stems. Callers map `None` onto their own error type.
    pub fn file_stem(&self) -> Option<String> {
        // Encoding these would produce a valid but confusing name, and no real
        // agent emits them; refusing is the honest answer for an id that is
        // only ever a relative path.
        if self.0 == "." || self.0 == ".." {
            return None;
        }
        let mut out = String::with_capacity(self.0.len());
        for byte in self.0.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => {
                    out.push(byte as char)
                }
                _ => {
                    use std::fmt::Write as _;
                    write!(out, "%{byte:02X}").expect("writing to a String cannot fail");
                }
            }
        }
        Some(out)
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Identifies an event within one session.
///
/// Agents disagree about event identity: Claude Code stamps every line with a
/// `uuid`, Codex stamps nothing and relies on file order. Both are representable
/// here, and the adapter decides which it can honestly provide.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventId {
    /// The agent supplied a stable identifier.
    Uuid(String),
    /// No identifier available; position in the file is the only identity.
    Ordinal(u32),
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EventId::Uuid(u) => f.write_str(u),
            EventId::Ordinal(n) => write!(f, "#{n}"),
        }
    }
}

/// A 1-based turn number, where a *turn* is one model request.
///
/// This definition is load-bearing and differs from the everyday sense of
/// "turn". The question ContextTrace answers -- "what was in the model's
/// context at this turn?" -- only has meaning per API call, so a turn is one
/// request/response pair, not one user exchange. A single user message that
/// triggers nine tool calls produces ten turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TurnNumber(u32);

impl TurnNumber {
    pub fn new(n: u32) -> Result<Self, IdentityError> {
        if n == 0 {
            return Err(IdentityError::ZeroTurn);
        }
        Ok(Self(n))
    }

    /// First turn of any session.
    pub const FIRST: TurnNumber = TurnNumber(1);

    pub fn get(&self) -> u32 {
        self.0
    }

    pub fn previous(&self) -> Option<TurnNumber> {
        (self.0 > 1).then(|| TurnNumber(self.0 - 1))
    }
}

impl fmt::Display for TurnNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Identifies a session file within one parse run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct FileId(pub u32);

/// Identifies a reconstructed context item, so the same logical item can be
/// tracked across turns for lifecycle and diff views ("AGENTS.md, turn 1 -> 48").
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ContextItemId(String);

impl ContextItemId {
    pub fn new(raw: impl Into<String>) -> Self {
        Self(raw.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContextItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A fixed-size identity for one model-visible content payload.
///
/// Adapters compute this while they already hold the parsed content. The
/// domain deliberately knows neither the hash algorithm nor the agent format;
/// it only needs equality. The bytes are never exported because the identity
/// is an implementation detail, not session data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ContentFingerprint([u8; 32]);

impl ContentFingerprint {
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    Blank(&'static str),
    ZeroTurn,
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IdentityError::Blank(what) => write!(f, "{what} cannot be blank"),
            IdentityError::ZeroTurn => write!(f, "turn numbers are 1-based; 0 is not a valid turn"),
        }
    }
}

impl std::error::Error for IdentityError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_rejects_blank() {
        assert!(SessionId::new("  ").is_err());
        assert!(SessionId::new("abc").is_ok());
    }

    #[test]
    fn session_id_prefix_match_is_case_insensitive() {
        let id = SessionId::new("A1084967-25D1-4DED").unwrap();
        assert!(id.matches_prefix("a108"));
        assert!(!id.matches_prefix("b108"));
    }

    #[test]
    fn turn_numbers_are_one_based() {
        assert!(TurnNumber::new(0).is_err());
        assert_eq!(TurnNumber::new(1).unwrap(), TurnNumber::FIRST);
        assert_eq!(TurnNumber::FIRST.previous(), None);
        assert_eq!(
            TurnNumber::new(2).unwrap().previous(),
            Some(TurnNumber::FIRST)
        );
    }
}
