//! # ContextTrace use cases
//!
//! Orchestrates domain services over ports. This crate names no concrete
//! adapter: it is handed [`AgentBinding`]s by the composition root and works
//! entirely through [`AgentAdapter`] and [`TokenEstimator`] trait objects.
//!
//! The consequence worth stating: everything here works unchanged for an agent
//! that does not exist yet.

pub mod diagnostics;
pub mod diff;
pub mod export;
pub mod growth;
pub mod lifecycle;
pub mod secrets;

use ct_domain::ports::{AgentAdapter, ExactRecount, PortError, RawEventSource, TokenEstimator};
use ct_domain::services::ratio::{self, DerivedRatio, TurnSample};
use ct_domain::services::TokenCalibrator;
use ct_domain::{
    AgentKind, AgentSession, CompactionDiff, ContextSnapshot, SessionDescriptor, SessionId,
    TokenCount, TurnNumber,
};
use std::collections::BTreeMap;
use std::fmt;

pub use ct_domain::services::DerivedRatio as SessionRatio;
pub use diagnostics::{Diagnostics, DriftReport, DriftType, ResidualSpike, UnreadableSession};
pub use diff::{
    compare, CategoryDelta, Comparability, Instrument, SessionDiff, Side, SideSummary, ToolDelta,
};
pub use export::{ExportRecord, SCHEMA_VERSION};
pub use growth::{timeline, Bucket, CompactionAt, GrowthPoint, GrowthTimeline, Jump};
pub use lifecycle::{Departure, ItemLifecycle, ItemRecord, LifecycleSweep, ResolveError};
pub use secrets::{ExportRedaction, ExportReport, SecretFinding, SecretKind, SecretScanReport};

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
    TurnOutOfRange {
        requested: u32,
        available: usize,
    },
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
    ///
    /// An id is unique within an agent but not across them, and this signature
    /// carries no agent to separate them by. Where two agents hold the same id
    /// the exact-match short circuit below returns whichever binding was wired
    /// first, so a caller that already knows the agent should say so through
    /// [`ContextTrace::resolve_in_agent`] rather than rely on that order.
    pub fn resolve(&self, id_or_prefix: &str) -> Result<ResolvedSession, AppError> {
        self.resolve_scoped(None, id_or_prefix)
    }

    /// Find one session by id or unambiguous id prefix within a single agent.
    ///
    /// The desktop's catalog rows carry both halves of a session's identity, so
    /// they can ask this question exactly. Scoping the search first is what
    /// makes the second of two colliding ids reachable at all: an unscoped
    /// lookup answers with the first agent's session every time, and the other
    /// one cannot be opened however it is asked for.
    pub fn resolve_in_agent(
        &self,
        agent: AgentKind,
        id_or_prefix: &str,
    ) -> Result<ResolvedSession, AppError> {
        self.resolve_scoped(Some(agent), id_or_prefix)
    }

    fn resolve_scoped(
        &self,
        agent: Option<AgentKind>,
        id_or_prefix: &str,
    ) -> Result<ResolvedSession, AppError> {
        let mut matches: Vec<ResolvedSession> = Vec::new();

        for (index, binding) in self.bindings.iter().enumerate() {
            if agent.is_some_and(|wanted| binding.adapter.agent() != wanted) {
                continue;
            }
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
        self.parse(self.resolve(id_or_prefix)?)
    }

    /// Resolve and fully parse a session known to belong to one agent.
    pub fn load_in_agent(
        &self,
        agent: AgentKind,
        id_or_prefix: &str,
    ) -> Result<(AgentSession, ResolvedSession), AppError> {
        self.parse(self.resolve_in_agent(agent, id_or_prefix)?)
    }

    /// Resolve and parse a session for analyses that compare or compress item
    /// content.
    ///
    /// Kept separate from [`ContextTrace::load`] so commands that only inspect
    /// event structure do not hash and compress every payload only to discard
    /// the result.
    pub fn load_with_content_analysis(
        &self,
        id_or_prefix: &str,
    ) -> Result<(AgentSession, ResolvedSession), AppError> {
        self.parse_with_content_analysis(self.resolve(id_or_prefix)?)
    }

    /// The content-analysis load, scoped to one agent.
    pub fn load_with_content_analysis_in_agent(
        &self,
        agent: AgentKind,
        id_or_prefix: &str,
    ) -> Result<(AgentSession, ResolvedSession), AppError> {
        self.parse_with_content_analysis(self.resolve_in_agent(agent, id_or_prefix)?)
    }

    fn parse(
        &self,
        resolved: ResolvedSession,
    ) -> Result<(AgentSession, ResolvedSession), AppError> {
        let session = self.bindings[resolved.binding]
            .adapter
            .load(&resolved.descriptor)?;
        Ok((session, resolved))
    }

    fn parse_with_content_analysis(
        &self,
        resolved: ResolvedSession,
    ) -> Result<(AgentSession, ResolvedSession), AppError> {
        let session = self.bindings[resolved.binding]
            .adapter
            .load_with_content_analysis(&resolved.descriptor)?;
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
    /// keep their values and the slack becomes residual.
    ///
    /// What that buys is narrower than it first looks, and the difference
    /// matters. On a turn where *every* item was measured, the residual is no
    /// longer estimation error -- but it is not "unlogged context" either. An
    /// exact count is the model-visible text of an item, deliberately not the
    /// item's whole footprint in the request: the field names, role markers and
    /// block structure around it are excluded, because counting serialized JSON
    /// is the mistake CT-012 exists to prevent. So the remainder is the tool
    /// schemas *plus* that framing. Measured on two fully-exact Codex turns it
    /// came to 10,218 and 9,630 tokens -- large, stable, and now attributable
    /// to something specific rather than to our own arithmetic.
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

    /// Structurally account for the literal history a supported agent replaced.
    /// Raw bytes are fetched only for this explicit request.
    pub fn compaction_diffs(
        &self,
        session: &AgentSession,
        binding: usize,
        raw: &dyn RawEventSource,
    ) -> Result<Vec<CompactionDiff>, AppError> {
        let adapter = &self.bindings[binding].adapter;
        let estimator = self.bindings[binding].estimator.as_ref();
        Ok(adapter.compaction_diffs(session, raw, estimator)?)
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
    ///
    /// `None` when no turn reported a usable size. This used to rank on
    /// `unwrap_or(0)` and so always answered *something*, which meant a session
    /// with no usage records got its last turn described as its peak.
    pub fn peak_turn(&self, session: &AgentSession) -> Option<TurnNumber> {
        session.peak_turn()
    }

    /// Name of the estimator backing a binding, for display.
    /// The estimator paired with an agent at composition.
    ///
    /// Exposed so a caller that has *not* fitted a session-specific ratio can
    /// pass the same instrument the default path would have used, rather than
    /// constructing a second one that happens to differ.
    pub fn binding_estimator(&self, binding: usize) -> &dyn TokenEstimator {
        self.bindings[binding].estimator.as_ref()
    }

    pub fn estimator_name(&self, binding: usize) -> &str {
        self.bindings[binding].estimator.name()
    }

    /// Run read-only diagnostics over a parsed session.
    /// Parse every discovered session and report what was not understood.
    ///
    /// `path_prefix` narrows the sweep to sessions whose file lives under a
    /// directory. Narrowing by *discovered* path rather than by walking an
    /// arbitrary directory is the point: the agent behind each file is then
    /// known from its descriptor, so nothing has to be guessed from a file's
    /// contents. Sniffing would mean inventing a detection rule, and a
    /// misdetected file reports as wholesale drift -- the loudest possible way
    /// for a guess to be wrong.
    ///
    /// One unreadable session never stops the sweep. Finding out that four
    /// hundred sessions are fine and one is not is the whole point of running
    /// this, and aborting on the first failure would report the opposite.
    pub fn sweep_drift(&self, path_prefix: Option<&str>) -> DriftReport {
        // Keyed by agent and raw type, because the same type name from two
        // agents is two different findings.
        let mut seen: BTreeMap<(AgentKind, String), (u32, usize, String)> = BTreeMap::new();
        let mut report = DriftReport {
            requested_prefix: path_prefix.map(str::to_string),
            ..Default::default()
        };

        for binding in &self.bindings {
            let agent = binding.adapter.agent();
            let Ok(descriptors) = binding.adapter.discover() else {
                continue;
            };
            let mut scanned = 0usize;

            for descriptor in descriptors {
                if !path_matches(&descriptor.path, path_prefix) {
                    continue;
                }
                scanned += 1;

                let session = match binding.adapter.load(&descriptor) {
                    Ok(session) => session,
                    Err(e) => {
                        report.unreadable.push(UnreadableSession {
                            id: descriptor.id.to_string(),
                            agent,
                            path: descriptor.path.clone(),
                            error: e.to_string(),
                        });
                        continue;
                    }
                };

                report.total_events += session.events().len();
                report.unrecognised_events += session.unrecognised_total();

                for (raw_type, count) in session.unrecognised() {
                    let entry = seen
                        .entry((agent, raw_type.clone()))
                        .or_insert_with(|| (0, 0, session.id().to_string()));
                    entry.0 += count;
                    entry.1 += 1;
                }
            }

            report.sessions_scanned += scanned;
            report.scanned_by_agent.push((agent, scanned));
        }

        report.types = seen
            .into_iter()
            .map(
                |((agent, raw_type), (events, sessions, example))| DriftType {
                    agent,
                    raw_type,
                    events,
                    sessions,
                    example,
                },
            )
            .collect();
        // Spread first, then volume: a type in every session is a shipped
        // format change, and one appearing many times in a single session is
        // more likely one long-running experiment.
        report
            .types
            .sort_by(|a, b| b.sessions.cmp(&a.sessions).then(b.events.cmp(&a.events)));

        report
    }

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

/// Turns either side of a step within which a compaction already explains it.
///
/// Deliberately a distinct constant from [`STEP_WINDOW`] even though both
/// happen to be `5` today. `STEP_WINDOW` is a *detection* window: how many
/// turns of history on each side of a candidate step feed the median that
/// decides whether the remainder actually moved. This is an *attribution*
/// window, applied afterwards: once a step is already found, how close a
/// recorded compaction has to sit to count as the reason for it. One measures
/// a series against itself; the other measures a series against a separate
/// list of events. Sharing a value is coincidence, not a shared purpose --
/// tuning how a step is detected is not a reason to move how a step is
/// explained, and conflating the two constants would silently couple
/// decisions that have nothing to do with each other.
pub const STEP_ATTRIBUTION_WINDOW: u32 = 5;

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

/// Is this session file under `prefix`?
///
/// Separators are normalised and case is folded, because on Windows a user who
/// types the path they see in `ct roots` will mix `\` and `/` and will not match
/// the drive letter's case. A prefix that matches nothing is reported as a
/// swept-nothing result rather than an error -- the sweep cannot tell a typo
/// from a directory that genuinely holds no sessions, and guessing which it was
/// would be worse than saying zero.
fn path_matches(path: &str, prefix: Option<&str>) -> bool {
    let Some(prefix) = prefix.filter(|p| !p.is_empty()) else {
        return true;
    };
    normalise_path(path).starts_with(&normalise_path(prefix))
}

fn normalise_path(path: &str) -> String {
    path.replace('\\', "/").to_lowercase()
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
    use ct_domain::ThreadRole;

    fn descriptor(id: &str, agent: AgentKind, project: &str, day: u32) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new(id).unwrap(),
            agent,
            path: format!("/tmp/{id}.jsonl"),
            size_bytes: 100,
            project: Some(project.into()),
            started_at: None,
            last_activity: Some(Utc.with_ymd_and_hms(2026, 7, day, 0, 0, 0).unwrap()),
            thread_role: ThreadRole::Root,
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

    #[test]
    fn a_path_prefix_survives_the_separator_and_case_a_user_will_type() {
        let path = r"C:\Users\anesk\.codex\sessions\2026\07\rollout.jsonl";
        assert!(path_matches(path, Some("c:/users/anesk/.codex")));
        assert!(path_matches(
            path,
            Some(r"C:\Users\anesk\.codex\sessions\2026")
        ));
        assert!(!path_matches(path, Some(r"C:\Users\anesk\.claude")));
    }

    #[test]
    fn an_absent_or_empty_prefix_sweeps_everything() {
        // `--dir` with no value means "everywhere", which clap gives us as an
        // empty string rather than as `None`.
        let path = "/home/x/.codex/s.jsonl";
        assert!(path_matches(path, None));
        assert!(path_matches(path, Some("")));
    }

    /// Serves a fixed set of sessions, so the sweep's aggregation can be tested
    /// without a corpus on disk.
    struct FakeAdapter {
        agent: AgentKind,
        sessions: Vec<(SessionDescriptor, Result<AgentSession, ()>)>,
    }

    impl ct_domain::ports::AgentAdapter for FakeAdapter {
        fn agent(&self) -> AgentKind {
            self.agent
        }
        fn roots(&self) -> Vec<String> {
            Vec::new()
        }
        fn discover(&self) -> ct_domain::ports::PortResult<Vec<SessionDescriptor>> {
            Ok(self.sessions.iter().map(|(d, _)| d.clone()).collect())
        }
        fn load(&self, wanted: &SessionDescriptor) -> ct_domain::ports::PortResult<AgentSession> {
            let found = self
                .sessions
                .iter()
                .find(|(d, _)| d.id == wanted.id)
                .expect("test asked for a session it did not register");
            match &found.1 {
                Ok(session) => Ok(session.clone()),
                Err(()) => Err(PortError::Io("disk on fire".into())),
            }
        }
        fn reconstruct(
            &self,
            _: &AgentSession,
            _: TurnNumber,
            _: &dyn TokenEstimator,
        ) -> ct_domain::ports::PortResult<ct_domain::ports::ReconstructedContext> {
            unimplemented!("the sweep never reconstructs")
        }
    }

    fn swept_session(
        id: &str,
        agent: AgentKind,
        events: usize,
        unrecognised: Vec<(&str, u32)>,
    ) -> AgentSession {
        use ct_domain::model::event::EventLinks;
        use ct_domain::{Event, EventId, EventKind, FileId, SessionMetadata, SourceRef};

        let events = (0..events)
            .map(|i| Event {
                id: EventId::Ordinal(i as u32),
                sequence: i as u32,
                timestamp: None,
                kind: EventKind::SessionStarted,
                source: SourceRef::new(FileId(0), 0, 0, i as u32 + 1),
                raw_type: "x".into(),
                turn: None,
                links: EventLinks::default(),
                content_measurement: None,
            })
            .collect();

        AgentSession::new(
            SessionId::new(id).unwrap(),
            agent,
            SessionMetadata::default(),
            events,
            vec![],
            unrecognised
                .into_iter()
                .map(|(t, n)| (t.to_string(), n))
                .collect(),
        )
    }

    fn sweeping(
        agent: AgentKind,
        sessions: Vec<(SessionDescriptor, Result<AgentSession, ()>)>,
    ) -> ContextTrace {
        ContextTrace::new(vec![AgentBinding::new(
            Box::new(FakeAdapter { agent, sessions }),
            Box::new(CharProbe),
        )])
    }

    /// Two agents legitimately holding one id is the case the two lookups exist
    /// to tell apart. Pinned as a test rather than left in a doc comment
    /// because the unscoped path is the one every CLI command still takes: its
    /// answer is a wiring order, not a disambiguation, and a later reader
    /// should find that asserted rather than described.
    #[test]
    fn an_unscoped_resolve_answers_with_the_first_binding_and_a_scoped_one_does_not() {
        let colliding = |agent| {
            vec![(
                descriptor("shared", agent, "p", 1),
                Ok(swept_session("shared", agent, 1, Vec::new())),
            )]
        };
        let app = ContextTrace::new(vec![
            AgentBinding::new(
                Box::new(FakeAdapter {
                    agent: AgentKind::ClaudeCode,
                    sessions: colliding(AgentKind::ClaudeCode),
                }),
                Box::new(CharProbe),
            ),
            AgentBinding::new(
                Box::new(FakeAdapter {
                    agent: AgentKind::Codex,
                    sessions: colliding(AgentKind::Codex),
                }),
                Box::new(CharProbe),
            ),
        ]);

        // Unchanged by the agent-scoped addition: with no agent to select by,
        // the exact-match short circuit returns the first binding that holds
        // the id, which is the behaviour every existing caller was built on.
        let unscoped = app.resolve("shared").expect("an exact id still resolves");
        assert_eq!(unscoped.descriptor.agent, AgentKind::ClaudeCode);
        assert_eq!(unscoped.binding, 0);

        // Scoping first is what reaches the later binding at all. Without it
        // the Codex session is unreachable however it is asked for.
        for (index, agent) in [(0, AgentKind::ClaudeCode), (1, AgentKind::Codex)] {
            let scoped = app
                .resolve_in_agent(agent, "shared")
                .expect("each agent resolves its own session");
            assert_eq!(scoped.descriptor.agent, agent);
            assert_eq!(scoped.binding, index);
        }
    }

    #[test]
    fn drift_counts_sessions_not_just_events() {
        // The figure that separates a one-off from a shipped format change.
        let app = sweeping(
            AgentKind::Codex,
            vec![
                (
                    descriptor("a", AgentKind::Codex, "p", 1),
                    Ok(swept_session(
                        "a",
                        AgentKind::Codex,
                        10,
                        vec![("new_type", 3)],
                    )),
                ),
                (
                    descriptor("b", AgentKind::Codex, "p", 2),
                    Ok(swept_session(
                        "b",
                        AgentKind::Codex,
                        10,
                        vec![("new_type", 1)],
                    )),
                ),
                (
                    descriptor("c", AgentKind::Codex, "p", 3),
                    Ok(swept_session("c", AgentKind::Codex, 10, vec![])),
                ),
            ],
        );

        let report = app.sweep_drift(None);
        assert_eq!(report.sessions_scanned, 3);
        assert_eq!(report.total_events, 30);
        assert_eq!(report.types.len(), 1);
        assert_eq!(report.types[0].events, 4);
        assert_eq!(report.types[0].sessions, 2);
        assert!(!report.is_clean());
    }

    #[test]
    fn one_unreadable_session_does_not_end_the_sweep() {
        // Finding out that the rest are fine is the entire reason to run this.
        let app = sweeping(
            AgentKind::Codex,
            vec![
                (descriptor("a", AgentKind::Codex, "p", 1), Err(())),
                (
                    descriptor("b", AgentKind::Codex, "p", 2),
                    Ok(swept_session("b", AgentKind::Codex, 5, vec![])),
                ),
            ],
        );

        let report = app.sweep_drift(None);
        assert_eq!(report.sessions_scanned, 2);
        assert_eq!(
            report.total_events, 5,
            "the readable session was still parsed"
        );
        assert_eq!(report.unreadable.len(), 1);
        assert!(
            report.types.is_empty(),
            "an unreadable file is not an unrecognised event type"
        );
        assert!(!report.is_clean(), "but it is still a reason to fail CI");
    }

    #[test]
    fn a_clean_sweep_reports_full_fidelity_rather_than_silence() {
        let app = sweeping(
            AgentKind::Codex,
            vec![(
                descriptor("a", AgentKind::Codex, "p", 1),
                Ok(swept_session("a", AgentKind::Codex, 12, vec![])),
            )],
        );

        let report = app.sweep_drift(None);
        assert!(report.is_clean());
        assert_eq!(report.fidelity(), 1.0);
        assert_eq!(report.scanned_by_agent, vec![(AgentKind::Codex, 1)]);
    }

    #[test]
    fn a_prefix_matching_nothing_is_reported_rather_than_read_as_clean() {
        // The sweep cannot tell a typo from a directory that genuinely holds no
        // sessions, so it does not try. What it must not do is let either one
        // pass as "this format was checked and is fine".
        let app = sweeping(
            AgentKind::Codex,
            vec![(
                descriptor("a", AgentKind::Codex, "p", 1),
                Ok(swept_session(
                    "a",
                    AgentKind::Codex,
                    5,
                    vec![("new_type", 1)],
                )),
            )],
        );

        let report = app.sweep_drift(Some("Z:/nowhere"));
        assert_eq!(report.sessions_scanned, 0);
        assert!(report.is_clean(), "nothing swept is not a drift finding");
        assert!(report.matched_nothing(), "but it is not a pass either");

        // Sweeping everywhere and finding nothing is a different situation: it
        // means this machine has no sessions, not that a path was wrong.
        let empty = ContextTrace::new(vec![AgentBinding::new(
            Box::new(FakeAdapter {
                agent: AgentKind::Codex,
                sessions: vec![],
            }),
            Box::new(CharProbe),
        )]);
        assert!(!empty.sweep_drift(None).matched_nothing());
    }
}
