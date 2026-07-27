//! # ContextTrace use cases
//!
//! Orchestrates domain services over ports. This crate names no concrete
//! adapter: it is handed [`AgentBinding`]s by the composition root and works
//! entirely through [`AgentAdapter`] and [`TokenEstimator`] trait objects.
//!
//! The consequence worth stating: everything here works unchanged for an agent
//! that does not exist yet.

pub mod diagnostics;

use ct_domain::ports::{AgentAdapter, PortError, TokenEstimator};
use ct_domain::services::ratio::{self, DerivedRatio, TurnSample};
use ct_domain::services::TokenCalibrator;
use ct_domain::{
    AgentKind, AgentSession, ContextSnapshot, SessionDescriptor, SessionId, TokenCount, TurnNumber,
};
use std::fmt;

pub use diagnostics::{Diagnostics, ResidualSpike};

/// An agent adapter paired with the token estimator appropriate to its models.
///
/// Pairing them is not incidental. Codex runs GPT-family models whose tokenizer
/// is public; Claude Code does not. Binding the estimator to the adapter means
/// the correct choice is made once, at composition, rather than being a
/// judgement call at every call site.
pub struct AgentBinding {
    pub adapter: Box<dyn AgentAdapter>,
    pub estimator: Box<dyn TokenEstimator>,
}

impl AgentBinding {
    pub fn new(adapter: Box<dyn AgentAdapter>, estimator: Box<dyn TokenEstimator>) -> Self {
        Self { adapter, estimator }
    }
}

#[derive(Debug)]
pub enum AppError {
    Port(PortError),
    /// No session matched the identifier the user gave.
    SessionNotFound(String),
    /// A prefix matched several sessions; the user must be more specific.
    AmbiguousSession {
        prefix: String,
        matches: Vec<String>,
    },
    /// The requested turn is outside the session.
    TurnOutOfRange { requested: u32, available: usize },
    Calibration(String),
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::Port(e) => write!(f, "{e}"),
            AppError::SessionNotFound(id) => write!(f, "no session matching '{id}'"),
            AppError::AmbiguousSession { prefix, matches } => write!(
                f,
                "'{prefix}' matches {} sessions: {}. Use a longer prefix.",
                matches.len(),
                matches.join(", ")
            ),
            AppError::TurnOutOfRange {
                requested,
                available,
            } => write!(
                f,
                "turn {requested} is out of range; this session has {available} turn(s)"
            ),
            AppError::Calibration(m) => write!(f, "calibration failed: {m}"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<PortError> for AppError {
    fn from(e: PortError) -> Self {
        AppError::Port(e)
    }
}

/// Criteria for narrowing a session listing.
#[derive(Debug, Default, Clone)]
pub struct SessionFilter {
    pub agent: Option<AgentKind>,
    /// Case-insensitive substring match against the project path.
    pub project: Option<String>,
    /// Sessions last active before this date are excluded.
    pub since: Option<chrono::NaiveDate>,
    pub limit: Option<usize>,
}

/// A session plus the index of the binding that can read it.
pub struct ResolvedSession {
    pub descriptor: SessionDescriptor,
    pub binding: usize,
}

/// The application service. One instance wires every supported agent.
pub struct ContextTrace {
    bindings: Vec<AgentBinding>,
}

impl ContextTrace {
    pub fn new(bindings: Vec<AgentBinding>) -> Self {
        Self { bindings }
    }

    /// Every local directory that will be read.
    ///
    /// Surfaced because ContextTrace should state plainly which paths it
    /// touches. Users handing a tool their raw agent logs are owed that, and it
    /// costs one line.
    pub fn roots(&self) -> Vec<(AgentKind, Vec<String>)> {
        self.bindings
            .iter()
            .map(|b| (b.adapter.agent(), b.adapter.roots()))
            .collect()
    }

    /// List sessions across all agents, newest first.
    ///
    /// A failing adapter is skipped rather than fatal: one agent's directory
    /// being unreadable must not deny the user the other agent's sessions.
    pub fn list_sessions(&self, filter: &SessionFilter) -> Vec<SessionDescriptor> {
        let mut out: Vec<SessionDescriptor> = self
            .bindings
            .iter()
            .filter(|b| filter.agent.is_none() || filter.agent == Some(b.adapter.agent()))
            .filter_map(|b| b.adapter.discover().ok())
            .flatten()
            .filter(|d| matches_filter(d, filter))
            .collect();

        out.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        out
    }

    /// Find one session by id or unambiguous id prefix.
    pub fn resolve(&self, id_or_prefix: &str) -> Result<ResolvedSession, AppError> {
        let mut matches: Vec<ResolvedSession> = Vec::new();

        for (index, binding) in self.bindings.iter().enumerate() {
            let Ok(sessions) = binding.adapter.discover() else {
                continue;
            };
            for descriptor in sessions {
                // An exact match wins outright, even when it is also a prefix
                // of some longer id.
                if descriptor.id.as_str() == id_or_prefix {
                    return Ok(ResolvedSession {
                        descriptor,
                        binding: index,
                    });
                }
                if descriptor.id.matches_prefix(id_or_prefix) {
                    matches.push(ResolvedSession {
                        descriptor,
                        binding: index,
                    });
                }
            }
        }

        match matches.len() {
            0 => Err(AppError::SessionNotFound(id_or_prefix.to_string())),
            1 => Ok(matches.remove(0)),
            _ => Err(AppError::AmbiguousSession {
                prefix: id_or_prefix.to_string(),
                matches: matches
                    .iter()
                    .take(5)
                    .map(|m| m.descriptor.id.to_string())
                    .collect(),
            }),
        }
    }

    /// Resolve and fully parse a session.
    pub fn load(&self, id_or_prefix: &str) -> Result<(AgentSession, ResolvedSession), AppError> {
        let resolved = self.resolve(id_or_prefix)?;
        let session = self.bindings[resolved.binding]
            .adapter
            .load(&resolved.descriptor)?;
        Ok((session, resolved))
    }

    /// Reconstruct and calibrate the context at a turn.
    ///
    /// The two-step shape is deliberate: the adapter reconstructs (it knows its
    /// agent's semantics) and the domain calibrates (it owns the rule that the
    /// numbers must add up). Neither can do the other's job, so a future
    /// adapter cannot publish an unbalanced breakdown.
    pub fn snapshot(
        &self,
        session: &AgentSession,
        binding: usize,
        turn: TurnNumber,
    ) -> Result<ContextSnapshot, AppError> {
        if session.turn(turn).is_none() {
            return Err(AppError::TurnOutOfRange {
                requested: turn.get(),
                available: session.turn_count(),
            });
        }

        let estimator = self.bindings[binding].estimator.as_ref();
        self.snapshot_with(session, binding, turn, estimator)
    }

    /// As [`ContextTrace::snapshot`], but with an estimator chosen by the
    /// caller.
    ///
    /// Exists so the composition root can substitute an estimator calibrated to
    /// *this* session -- see [`ContextTrace::derive_ratio`]. The use case is
    /// otherwise identical, and the domain still owns the balancing rule.
    pub fn snapshot_with(
        &self,
        session: &AgentSession,
        binding: usize,
        turn: TurnNumber,
        estimator: &dyn TokenEstimator,
    ) -> Result<ContextSnapshot, AppError> {
        if session.turn(turn).is_none() {
            return Err(AppError::TurnOutOfRange {
                requested: turn.get(),
                available: session.turn_count(),
            });
        }

        let reconstructed = self.bindings[binding]
            .adapter
            .reconstruct(session, turn, estimator)?;

        TokenCalibrator::calibrate(reconstructed, session.id().clone(), session.agent(), turn)
            .map_err(|e| AppError::Calibration(e.to_string()))
    }

    /// Measure this session's characters-per-token from its own usage figures.
    ///
    /// Reconstructs every turn with an estimator that returns character counts
    /// unchanged, giving the domain the `(chars, tokens)` pairs it needs. The
    /// probe is why this belongs in the application layer: it is one use case
    /// composing the reconstruction port with a domain service, and neither half
    /// has to know the other exists.
    ///
    /// Returns `None` when the session lacks enough growth to measure, which is
    /// normal for short sessions and not an error.
    pub fn derive_ratio(&self, session: &AgentSession, binding: usize) -> Option<DerivedRatio> {
        let probe = CharProbe;
        let adapter = &self.bindings[binding].adapter;

        let samples: Vec<TurnSample> = session
            .turns()
            .iter()
            .filter_map(|turn| {
                let tokens = turn.prompt_tokens()?;
                let context = adapter.reconstruct(session, turn.number, &probe).ok()?;
                Some(TurnSample {
                    chars: context.items.iter().map(|i| i.tokens.tokens() as u64).sum(),
                    tokens,
                    depth: context.items.len().min(u32::MAX as usize) as u32,
                })
            })
            .collect();

        ratio::derive(&samples)
    }

    /// The turn with the largest prompt -- usually where to start looking.
    pub fn peak_turn(&self, session: &AgentSession) -> Option<TurnNumber> {
        session
            .turns()
            .iter()
            .max_by_key(|t| t.prompt_tokens().unwrap_or(0))
            .map(|t| t.number)
    }

    /// Name of the estimator backing a binding, for display.
    pub fn estimator_name(&self, binding: usize) -> &str {
        self.bindings[binding].estimator.name()
    }

    /// Run read-only diagnostics over a parsed session.
    pub fn diagnose(&self, session: &AgentSession) -> Diagnostics {
        diagnostics::diagnose(session)
    }

    pub fn session_id_of<'a>(&self, session: &'a AgentSession) -> &'a SessionId {
        session.id()
    }
}

/// An estimator that reports characters as-is.
///
/// Not a token estimate and never presented as one -- it is a measuring
/// instrument. Reconstructing a turn through it makes the reconstruction report
/// how many *characters* it accounted for, which is the input the ratio service
/// needs. Keeping it private prevents it leaking into anything user-facing.
struct CharProbe;

impl TokenEstimator for CharProbe {
    fn count_text(&self, text: &str) -> TokenCount {
        TokenCount::estimated(text.chars().count().min(u32::MAX as usize) as u32)
    }

    fn estimate_from_chars(&self, char_len: u32) -> TokenCount {
        TokenCount::estimated(char_len)
    }

    fn name(&self) -> &str {
        "characters"
    }
}

fn matches_filter(descriptor: &SessionDescriptor, filter: &SessionFilter) -> bool {
    if let Some(agent) = filter.agent {
        if descriptor.agent != agent {
            return false;
        }
    }
    if let Some(needle) = &filter.project {
        let haystack = descriptor.project.as_deref().unwrap_or_default();
        if !haystack.to_lowercase().contains(&needle.to_lowercase()) {
            return false;
        }
    }
    if let Some(since) = filter.since {
        match descriptor.last_activity {
            Some(ts) if ts.date_naive() >= since => {}
            // A session with no timestamp is kept: excluding it would silently
            // hide data because the agent omitted a field.
            None => {}
            _ => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{NaiveDate, TimeZone, Utc};

    fn descriptor(id: &str, agent: AgentKind, project: &str, day: u32) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new(id).unwrap(),
            agent,
            path: format!("/tmp/{id}.jsonl"),
            size_bytes: 100,
            project: Some(project.into()),
            started_at: None,
            last_activity: Some(Utc.with_ymd_and_hms(2026, 7, day, 0, 0, 0).unwrap()),
        }
    }

    #[test]
    fn project_filter_is_case_insensitive_substring() {
        let d = descriptor("a", AgentKind::Codex, "C:\\repos\\ContextTrace", 10);
        let hit = SessionFilter {
            project: Some("contexttrace".into()),
            ..Default::default()
        };
        assert!(matches_filter(&d, &hit));

        let miss = SessionFilter {
            project: Some("other".into()),
            ..Default::default()
        };
        assert!(!matches_filter(&d, &miss));
    }

    #[test]
    fn agent_filter_excludes_other_agents() {
        let d = descriptor("a", AgentKind::Codex, "p", 10);
        let f = SessionFilter {
            agent: Some(AgentKind::ClaudeCode),
            ..Default::default()
        };
        assert!(!matches_filter(&d, &f));
    }

    #[test]
    fn since_filter_boundary_date_is_inclusive() {
        let d = descriptor("a", AgentKind::Codex, "p", 10);
        let same_day = SessionFilter {
            since: NaiveDate::from_ymd_opt(2026, 7, 10),
            ..Default::default()
        };
        assert!(matches_filter(&d, &same_day));

        let later = SessionFilter {
            since: NaiveDate::from_ymd_opt(2026, 7, 11),
            ..Default::default()
        };
        assert!(!matches_filter(&d, &later));
    }

    #[test]
    fn sessions_without_a_timestamp_survive_a_date_filter() {
        let mut d = descriptor("a", AgentKind::Codex, "p", 10);
        d.last_activity = None;
        let f = SessionFilter {
            since: NaiveDate::from_ymd_opt(2026, 7, 20),
            ..Default::default()
        };
        assert!(
            matches_filter(&d, &f),
            "a missing field must not silently hide a session"
        );
    }
}
