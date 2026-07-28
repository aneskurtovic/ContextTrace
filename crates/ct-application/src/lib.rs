//! # ContextTrace use cases
//!
//! Orchestrates domain services over ports. This crate names no concrete
//! adapter: it is handed [`AgentBinding`]s by the composition root and works
//! entirely through [`AgentAdapter`] and [`TokenEstimator`] trait objects.
//!
//! The consequence worth stating: everything here works unchanged for an agent
//! that does not exist yet.

pub mod diagnostics;
pub mod lifecycle;

use ct_domain::ports::{AgentAdapter, ExactRecount, PortError, RawEventSource, TokenEstimator};
use ct_domain::services::ratio::{self, DerivedRatio, TurnSample};
use ct_domain::services::TokenCalibrator;
use ct_domain::{
    AgentKind, AgentSession, ContextSnapshot, SessionDescriptor, SessionId, TokenCount, TurnNumber,
};
use std::fmt;

pub use diagnostics::{Diagnostics, ResidualSpike};
pub use lifecycle::{Departure, ItemLifecycle, ItemRecord, LifecycleSweep, ResolveError};
pub use ct_domain::services::DerivedRatio as SessionRatio;

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

    /// As [`ContextTrace::snapshot`], but measuring each item instead of
    /// estimating it, where the agent allows that.
    pub fn snapshot_exact(
        &self,
        session: &AgentSession,
        binding: usize,
        turn: TurnNumber,
        raw: &dyn RawEventSource,
    ) -> Result<(ContextSnapshot, ExactRecount), AppError> {
        let estimator = self.bindings[binding].estimator.as_ref();
        self.snapshot_exact_with(session, binding, turn, estimator, raw)
    }

    /// As [`ContextTrace::snapshot_with`], but measuring each item instead of
    /// estimating it, where the agent allows that.
    ///
    /// Costs one seek, one JSON parse and one tokenizer pass per context item,
    /// which is why it is a separate use case rather than the default. The
    /// [`ExactRecount`] comes back beside the snapshot because how much was
    /// actually measured is part of the answer -- an exact view in which most
    /// items stayed estimated is a different claim from one in which they did
    /// not, and the caller must be able to say which it has.
    ///
    /// Returns [`PortError::Unsupported`] through [`AppError`] for an agent
    /// that cannot do this, rather than silently returning estimates.
    ///
    /// Calibration afterwards is unchanged and needs no special case: its first
    /// rule is already that measured counts are never rescaled, so exact items
    /// keep their values and the slack becomes residual. That is the point of
    /// the whole exercise -- for Codex, whose system prompt *is* logged, the
    /// remainder stops being "unlogged context plus our estimation error" and
    /// becomes just the first.
    pub fn snapshot_exact_with(
        &self,
        session: &AgentSession,
        binding: usize,
        turn: TurnNumber,
        estimator: &dyn TokenEstimator,
        raw: &dyn RawEventSource,
    ) -> Result<(ContextSnapshot, ExactRecount), AppError> {
        if session.turn(turn).is_none() {
            return Err(AppError::TurnOutOfRange {
                requested: turn.get(),
                available: session.turn_count(),
            });
        }

        let adapter = &self.bindings[binding].adapter;
        let mut reconstructed = adapter.reconstruct(session, turn, estimator)?;
        let report = adapter.recount_exact(session, &mut reconstructed.items, raw, estimator)?;

        let snapshot =
            TokenCalibrator::calibrate(reconstructed, session.id().clone(), session.agent(), turn)
                .map_err(|e| AppError::Calibration(e.to_string()))?;

        Ok((snapshot, report))
    }

    /// Per-turn history of what the log could and could not account for.
    ///
    /// The unlogged remainder is stable by nature -- an agent's system prompt
    /// and tool schemas do not change while a session runs -- so a step change
    /// in it means the harness altered them mid-session. Nothing in the log
    /// records that happening, which is why watching the remainder is the only
    /// way to see it.
    ///
    /// Reconstructs each turn once through a character probe, so the cost is one
    /// pass over the session rather than one per turn per view.
    pub fn residual_series(
        &self,
        session: &AgentSession,
        binding: usize,
        ratio: DerivedRatio,
    ) -> Vec<ResidualPoint> {
        let probe = CharProbe;
        let adapter = &self.bindings[binding].adapter;

        session
            .turns()
            .iter()
            .filter_map(|turn| {
                let tokens = turn.prompt_tokens()?;
                let context = adapter.reconstruct(session, turn.number, &probe).ok()?;
                let chars: u64 = context.items.iter().map(|i| i.tokens.tokens() as u64).sum();
                Some(ResidualPoint {
                    turn: turn.number.get(),
                    prompt_tokens: tokens,
                    accounted: ratio.accounted_at(chars),
                    unlogged: ratio.unlogged_at(chars, tokens),
                    items: context.items.len(),
                })
            })
            .collect()
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

/// One turn's account of its own prompt.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ResidualPoint {
    pub turn: u32,
    /// The agent's observed prompt size.
    pub prompt_tokens: u32,
    /// Tokens the reconstructed content accounts for.
    pub accounted: u32,
    /// Tokens the log cannot account for: the system prompt and tool schemas.
    ///
    /// `None` where reconstruction over-counted, which is reported as unknown
    /// rather than zero -- a zero here would assert a complete inventory.
    pub unlogged: Option<u32>,
    pub items: usize,
}

/// A change in the unlogged remainder large enough to mean the harness altered
/// the prompt's hidden part.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ResidualStep {
    pub turn: u32,
    pub from: u32,
    pub to: u32,
}

impl ResidualStep {
    pub fn growth(&self) -> i64 {
        self.to as i64 - self.from as i64
    }
}

/// Smallest sustained change in the unlogged remainder worth reporting.
pub const RESIDUAL_STEP_THRESHOLD: i64 = 5_000;

/// Turns either side of a candidate step used to establish its levels.
const STEP_WINDOW: usize = 5;

/// Find *sustained* changes in the unlogged remainder.
///
/// # Why comparing consecutive turns does not work
///
/// The remainder is not the clean constant it ought to be: it drifts upward over
/// a session, because one fitted ratio cannot describe a content mix that starts
/// as prose and becomes dominated by tool output. Whatever the ratio gets wrong
/// lands here. Differencing adjacent turns therefore measures that drift, and on
/// a real session it flags nearly every turn -- which is no signal at all.
///
/// What separates a real change from drift is **persistence**. A registered tool
/// stays registered and an MCP server stays connected, so the remainder steps to
/// a new level and holds. Fit wobble reverts within a turn or two.
///
/// So a step is reported only when the median of the [`STEP_WINDOW`] turns
/// before differs from the median of the [`STEP_WINDOW`] turns after by at least
/// [`RESIDUAL_STEP_THRESHOLD`]. Medians because a single anomalous turn should
/// not create or mask a step.
///
/// Turns whose remainder is unknown are excluded from the windows rather than
/// skipped over silently; a window without enough known turns yields no step.
pub fn residual_steps(series: &[ResidualPoint]) -> Vec<ResidualStep> {
    let known: Vec<(u32, u32)> = series
        .iter()
        .filter_map(|p| p.unlogged.map(|u| (p.turn, u)))
        .collect();

    if known.len() < STEP_WINDOW * 2 {
        return Vec::new();
    }

    let mut steps: Vec<ResidualStep> = Vec::new();
    for index in STEP_WINDOW..=known.len() - STEP_WINDOW {
        let before = median_of(&known[index - STEP_WINDOW..index]);
        let after = median_of(&known[index..index + STEP_WINDOW]);
        let growth = after as i64 - before as i64;
        if growth.abs() < RESIDUAL_STEP_THRESHOLD {
            continue;
        }
        let step = ResidualStep {
            turn: known[index].0,
            from: before,
            to: after,
        };
        // One change produces a step at several neighbouring offsets. Keep the
        // largest of each run rather than reporting the same event repeatedly.
        match steps.last_mut() {
            Some(last)
                if step.turn.saturating_sub(last.turn) <= STEP_WINDOW as u32
                    && last.growth().signum() == growth.signum() =>
            {
                if growth.abs() > last.growth().abs() {
                    *last = step;
                }
            }
            _ => steps.push(step),
        }
    }
    steps
}

fn median_of(window: &[(u32, u32)]) -> u32 {
    let mut values: Vec<u32> = window.iter().map(|(_, v)| *v).collect();
    values.sort_unstable();
    match values.len() {
        0 => 0,
        n if n % 2 == 1 => values[n / 2],
        n => (values[n / 2 - 1] + values[n / 2]) / 2,
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

    fn point(turn: u32, unlogged: Option<u32>) -> ResidualPoint {
        ResidualPoint {
            turn,
            prompt_tokens: 100_000,
            accounted: 60_000,
            unlogged,
            items: 10,
        }
    }

    /// A remainder that drifts upward the way a real session's does.
    fn drifting(turns: u32, per_turn: u32) -> Vec<ResidualPoint> {
        (1..=turns)
            .map(|t| point(t, Some(30_000 + t * per_turn)))
            .collect()
    }

    #[test]
    fn ordinary_drift_is_not_reported_as_a_change() {
        // The remainder climbs ~48,000 tokens across this session, because one
        // fitted ratio cannot describe a changing content mix. Differencing
        // adjacent turns flagged nearly every turn here, which is no signal.
        let steps = residual_steps(&drifting(60, 800));
        assert!(
            steps.is_empty(),
            "steady drift is the ratio misfitting, not the harness changing: {steps:?}"
        );
    }

    #[test]
    fn a_sustained_jump_is_reported() {
        let mut series = drifting(40, 50);
        for p in series.iter_mut().filter(|p| p.turn > 20) {
            p.unlogged = p.unlogged.map(|u| u + 12_000);
        }
        let steps = residual_steps(&series);
        assert_eq!(steps.len(), 1, "expected exactly one step, got {steps:?}");
        assert!(
            steps[0].turn.abs_diff(21) <= 5,
            "step located at turn {}",
            steps[0].turn
        );
        assert!(steps[0].growth() > 10_000);
    }

    #[test]
    fn a_one_turn_spike_is_not_a_sustained_change() {
        // A single anomalous turn must not create a step: a registered tool
        // stays registered, so a real change holds its new level.
        let mut series = drifting(40, 50);
        series[20].unlogged = Some(200_000);
        assert!(
            residual_steps(&series).is_empty(),
            "a lone outlier is not a sustained change"
        );
    }

    #[test]
    fn one_change_is_reported_once_not_at_every_offset() {
        let mut series = drifting(40, 50);
        for p in series.iter_mut().filter(|p| p.turn > 20) {
            p.unlogged = p.unlogged.map(|u| u + 30_000);
        }
        assert_eq!(
            residual_steps(&series).len(),
            1,
            "the same event must not be reported several times"
        );
    }

    #[test]
    fn turns_with_an_unknown_remainder_do_not_fabricate_a_step() {
        // Over-counted turns have no measurable remainder. Treating them as
        // zero would invent an enormous fall and then an enormous rise.
        let mut series = drifting(40, 50);
        for p in series.iter_mut().filter(|p| (18..=22).contains(&p.turn)) {
            p.unlogged = None;
        }
        assert!(residual_steps(&series).is_empty());
    }

    #[test]
    fn a_short_session_yields_no_steps_rather_than_noise() {
        assert!(residual_steps(&drifting(6, 500)).is_empty());
        assert!(residual_steps(&[]).is_empty());
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
