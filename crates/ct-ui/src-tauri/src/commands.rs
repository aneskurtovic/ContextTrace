use ct_application::{
    residual_steps, timeline, AppError, Comparability, ContextTrace, CostCategory, CostForecast,
    CostReport, CostTurn, Departure, ExportRedaction, GhostItem, InstructionFileComparison,
    InstructionFileReport, InstructionFileStatus, LifecycleSweep, ResidualPoint, SessionDiff,
    SessionFilter, SessionSource, TemporalGhost, UnpricedTurn, RESIDUAL_STEP_THRESHOLD,
    STEP_ATTRIBUTION_WINDOW,
};
use ct_domain::model::archive::{ArchiveEntry, ArchiveIntegrity, RedactionMode};
use ct_domain::model::context::{unmeasured_content_items, ContextItem};
use ct_domain::ports::{ArchiveStore, PortError, TokenEstimator};
use ct_domain::{
    AgentKind, CategoryBreakdown, CompactionDiff, CompactionDiffItem, CompactionDiffUnavailable,
    CompactionItemDisposition, Confidence, ContextItemId, ContextSource, MessageRole,
    SessionDescriptor, ThreadRole, TurnNumber,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::{AppHandle, Emitter};

pub mod notifications;

const DEFAULT_SESSION_PAGE_SIZE: usize = 200;
const MAX_SESSION_PAGE_SIZE: usize = 1_000;
/// Smaller than a page of the catalog, and for a different reason: a catalog
/// row is a few fields, while a transcript entry carries up to
/// [`ct_application::transcript::PAGE_TEXT_CHARS`] of text apiece.
const DEFAULT_TRANSCRIPT_PAGE_SIZE: usize = 40;
const MAX_TRANSCRIPT_PAGE_SIZE: usize = 200;

/// How many parsed sessions the desktop keeps warm at once.
///
/// Each entry holds an entire parsed `AgentSession` plus its calibration
/// ratio, not a lightweight summary. The realistic case is a user comparing a
/// handful of sessions in one sitting, not browsing the full local catalog
/// (hundreds of sessions on a real machine) and expecting every one of them
/// to stay parsed for the life of the process. Eight slots covers that
/// realistic back-and-forth — the current session plus a few just-visited
/// ones — without retaining the whole history.
const SESSION_CACHE_CAPACITY: usize = 8;

/// How many per-item lifecycle sweeps the desktop keeps warm at once.
///
/// A sweep walks every turn of one session once; it is cheap to redo and
/// only ever serves the session currently open, so it gets the same small
/// cap as the session cache and for the same reason.
const LIFECYCLE_CACHE_CAPACITY: usize = 8;

/// Sessions are identified by agent and id together: the same id string can
/// legitimately appear under two different agents, and an id alone is not a
/// safe cache or lookup key.
type SessionKey = (AgentKind, String);

/// A capacity-bounded cache that evicts the least-recently-used entry.
///
/// Insertion order is the eviction order by default; `get` promotes an entry
/// to most-recently-used so a session someone keeps returning to survives
/// while ones only glanced at age out first.
struct BoundedCache<K, V> {
    capacity: usize,
    entries: HashMap<K, V>,
    // Front = least recently used, back = most recently used.
    order: VecDeque<K>,
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> BoundedCache<K, V> {
    fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        let value = self.entries.get(key).cloned();
        if value.is_some() {
            self.touch(key);
        }
        value
    }

    fn insert(&mut self, key: K, value: V) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key.clone(), value);
            self.touch(&key);
            return;
        }
        while self.entries.len() >= self.capacity {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        self.order.push_back(key.clone());
        self.entries.insert(key, value);
    }

    fn touch(&mut self, key: &K) {
        if let Some(pos) = self.order.iter().position(|existing| existing == key) {
            let existing = self.order.remove(pos).expect("position was just found");
            self.order.push_back(existing);
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        if let Some(pos) = self.order.iter().position(|existing| existing == key) {
            self.order.remove(pos);
        }
        self.entries.remove(key)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Caches hold `Arc` handles rather than values so a command can take what it
/// needs and release the lock before analysing anything. Tauri dispatches
/// commands on separate threads, and holding either lock across a lifecycle
/// sweep or the doctor's raw-file scan would serialise the whole desktop behind
/// whichever command is slowest.
pub struct AppState {
    app: ContextTrace,
    warnings: Vec<String>,
    /// Production owns the shared archive; fixture/test construction leaves
    /// this disabled so a developer's real archive cannot contaminate tests or
    /// synthetic catalogs.
    archive: Option<ct_runtime::FileArchiveStore>,
    sessions: Mutex<BoundedCache<SessionKey, Arc<CachedSession>>>,
    lifecycles: Mutex<BoundedCache<SessionKey, Arc<LifecycleSweep>>>,
    /// The last corpus sweep, beside the corpus it was computed from.
    ///
    /// One entry, not a bounded cache: there is exactly one local corpus, and
    /// a sweep of it takes seconds. Re-running that every time the view is
    /// opened is the difference between a summary people check and one they
    /// avoid.
    corpus: Mutex<
        Option<(
            Vec<ct_domain::SessionFingerprint>,
            ct_application::CorpusReport,
        )>,
    >,
}

struct CachedSession {
    session: ct_domain::AgentSession,
    descriptor: SessionDescriptor,
    binding: usize,
    source: SessionSource,
    /// The full derived ratio, not just its chars-per-token figure.
    ///
    /// `derive_ratio` reconstructs every turn of the session through a
    /// character probe, so it is one sweep no matter how much of the result a
    /// caller ends up using. Caching only `chars_per_token` was fine while
    /// every view needed just that field, but the residual report also needs
    /// `pairs_used`, `dispersion` and `unlogged_overhead` -- and re-deriving
    /// those on demand would mean a second full sweep per view rather than
    /// per session load. Keeping the whole ratio here means every current and
    /// future caller reads one sweep's answer instead of paying for their own.
    ratio: Option<ct_application::SessionRatio>,
    content_analyzed: bool,
}

impl CachedSession {
    /// The figure most callers actually want, out of the cached ratio.
    fn chars_per_token(&self) -> Option<f32> {
        self.ratio.map(|ratio| ratio.chars_per_token)
    }
}

impl AppState {
    pub fn new() -> Self {
        let runtime = ct_runtime::build();
        Self::from_parts_with_archive(
            runtime.app,
            runtime.warnings,
            Some(ct_runtime::archive_store()),
        )
    }

    #[cfg(test)]
    fn from_parts(app: ContextTrace, warnings: Vec<String>) -> Self {
        Self::from_parts_with_archive(app, warnings, None)
    }

    fn from_parts_with_archive(
        app: ContextTrace,
        warnings: Vec<String>,
        archive: Option<ct_runtime::FileArchiveStore>,
    ) -> Self {
        Self {
            app,
            warnings,
            archive,
            sessions: Mutex::new(BoundedCache::new(SESSION_CACHE_CAPACITY)),
            lifecycles: Mutex::new(BoundedCache::new(LIFECYCLE_CACHE_CAPACITY)),
            corpus: Mutex::new(None),
        }
    }

    /// Take a cache lock without letting one command's panic disable the rest.
    ///
    /// A poisoned mutex here only means some other command unwound while
    /// holding it. What these maps guard is a rebuildable cache of what is on
    /// disk, not state a half-finished write can leave inconsistent, so
    /// recovering the map beats failing every later command until the desktop
    /// is restarted.
    fn sessions(&self) -> MutexGuard<'_, BoundedCache<SessionKey, Arc<CachedSession>>> {
        self.sessions
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn lifecycles(&self) -> MutexGuard<'_, BoundedCache<SessionKey, Arc<LifecycleSweep>>> {
        self.lifecycles
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Invalidate only the session whose source file changed.
    ///
    /// The notification monitor watches every live descriptor. Clearing the
    /// whole cache on each 2.5-second poll would make an unrelated session the
    /// user is inspecting cold whenever any agent log grows. The agent-qualified
    /// key is the same identity used by both caches, so the parsed session and
    /// every lifecycle derived from it are discarded together.
    pub(crate) fn invalidate_session(&self, agent: AgentKind, id: &str) {
        let key = (agent, id.to_string());
        self.sessions().remove(&key);
        self.lifecycles().remove(&key);
    }

    /// Hand back a session handle that outlives the cache lock.
    ///
    /// Both the parse and every later analysis run with no lock held: the
    /// caller keeps the returned handle, so a doctor run over a large session
    /// cannot stop the session list from answering on another thread. Two
    /// threads may therefore load the same session at once, which costs a
    /// duplicate parse and never a wrong answer.
    ///
    /// `agent` scopes the lookup rather than merely checking it afterwards. An
    /// id is unique within an agent and not across them, and a catalog row
    /// carries both halves, so the backend is asked the question the row can
    /// actually answer. Resolving on the id alone would return whichever
    /// binding was wired first and leave the other session unreachable however
    /// it was clicked.
    fn cached_session(
        &self,
        agent: AgentKind,
        id: &str,
        analyzed: bool,
    ) -> Result<Arc<CachedSession>, String> {
        let key = (agent, id.to_string());
        if let Some(existing) = self.usable_cached_session(&key, analyzed) {
            return Ok(existing);
        }

        let loaded = match (&self.archive, analyzed) {
            (Some(archive), true) => self
                .app
                .load_with_content_analysis_in_agent_with_archive(agent, id, archive),
            (Some(archive), false) => self.app.load_in_agent_with_archive(agent, id, archive),
            (None, true) => self.app.load_with_content_analysis_in_agent(agent, id),
            (None, false) => self.app.load_in_agent(agent, id),
        };
        let (session, resolved) = loaded.map_err(|error| error.to_string())?;
        debug_assert_eq!(resolved.descriptor.agent, agent);
        let (_, ratio) = ct_runtime::calibrate_session(&self.app, &session, resolved.binding);
        let cached = Arc::new(CachedSession {
            session,
            descriptor: resolved.descriptor,
            binding: resolved.binding,
            source: resolved.source,
            ratio,
            content_analyzed: analyzed,
        });

        let mut sessions = self.sessions();
        // Another thread may have finished a content-analysed load while this
        // one was parsing; the richer entry stays.
        if let Some(existing) = sessions
            .get(&key)
            .filter(|existing| existing.content_analyzed && !analyzed)
        {
            return Ok(existing);
        }
        sessions.insert(key, Arc::clone(&cached));
        Ok(cached)
    }

    fn usable_cached_session(
        &self,
        key: &SessionKey,
        analyzed: bool,
    ) -> Option<Arc<CachedSession>> {
        self.sessions()
            .get(key)
            .filter(|cached| cached.content_analyzed || !analyzed)
    }

    fn startup(&self) -> StartupSummary {
        StartupSummary {
            roots: self
                .app
                .roots()
                .into_iter()
                .map(|(agent, paths)| RootSummary {
                    agent: agent.to_string(),
                    paths,
                })
                .collect(),
            warnings: self.warnings.clone(),
            // The startup panel names every directory ContextTrace reads; now
            // that the desktop can write too, the one directory it writes to
            // belongs in the same disclosure.
            archive_root: ct_runtime::archive_store().root(),
        }
    }

    /// Every descriptor a search matches, before paging.
    ///
    /// The query, the project and the thread role are all applied here, and all
    /// three after `list_sessions` rather than through `SessionFilter`: the
    /// query matches session ids and local source paths as well as projects, so
    /// narrowing the domain query by project would make an id search silently
    /// incomplete.
    fn filtered_descriptors(
        &self,
        agent: Option<String>,
        query: Option<String>,
        project: Option<&ProjectFilter>,
        include_subagents: bool,
    ) -> Result<Vec<SessionDescriptor>, String> {
        let parsed_agent = match agent.as_deref() {
            Some(agent) => Some(parse_agent(agent)?),
            None => None,
        };
        let filter = SessionFilter {
            agent: parsed_agent,
            project: None,
            since: None,
            limit: None,
        };
        let query = query
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let descriptors = match &self.archive {
            Some(archive) => self
                .app
                .list_sessions_with_archive(&filter, archive)
                .map_err(|error| error.to_string())?,
            None => self.app.list_sessions(&filter),
        };
        Ok(descriptors
            .into_iter()
            .filter(|descriptor| session_matches_query(descriptor, query.as_deref()))
            .filter(|descriptor| {
                include_subagents || matches!(descriptor.thread_role, ThreadRole::Root)
            })
            .filter(|descriptor| match project {
                None => true,
                Some(ProjectFilter::Unrecorded) => descriptor.project.is_none(),
                Some(ProjectFilter::Path(path)) => {
                    descriptor.project.as_deref() == Some(path.as_str())
                }
            })
            .collect())
    }

    /// Every project the catalog recognises, with the count each one would show.
    ///
    /// Counted over the whole filtered set rather than a page, and under the
    /// same agent, search query and subagent filters the list is showing, so
    /// the number beside an option always describes what selecting it does.
    fn list_projects(
        &self,
        agent: Option<String>,
        query: Option<String>,
        include_subagents: Option<bool>,
    ) -> Result<Vec<ProjectSummary>, String> {
        let descriptors =
            self.filtered_descriptors(agent, query, None, include_subagents.unwrap_or(false))?;
        let mut counts: BTreeMap<Option<String>, usize> = BTreeMap::new();
        for descriptor in &descriptors {
            *counts.entry(descriptor.project.clone()).or_default() += 1;
        }
        let mut projects: Vec<ProjectSummary> = counts
            .into_iter()
            .map(|(path, count)| ProjectSummary {
                label: match &path {
                    Some(path) => leaf_name(path),
                    // Not "unknown project": the log recorded no folder, which
                    // is a fact about the log, not a project called Unknown.
                    None => "No recorded folder".to_string(),
                },
                path,
                count,
            })
            .collect();
        projects.sort_by(|left, right| {
            right
                .count
                .cmp(&left.count)
                .then_with(|| left.label.cmp(&right.label))
        });
        Ok(projects)
    }

    /// Search the complete local catalog before taking a page.
    ///
    /// `refresh` distinguishes a genuine catalog refresh from an ordinary
    /// page request: the two need opposite cache policies. Paging through
    /// results the user already saw, or narrowing a query, must not discard
    /// sessions already parsed — that only wastes the parse the user just
    /// waited for. An explicit refresh means the caller wants to treat the
    /// cache as possibly stale (a file could have changed or disappeared on
    /// disk since it was cached), so only that case clears both caches.
    ///
    /// Seven parameters, not a params struct: every one of them is a wire
    /// argument the frontend names individually when it calls this command,
    /// so grouping them would only move the same fields behind a struct name
    /// the caller never sends -- it would not shrink the actual interface.
    #[allow(clippy::too_many_arguments)]
    fn search_sessions(
        &self,
        agent: Option<String>,
        query: Option<String>,
        project: Option<ProjectFilter>,
        include_subagents: Option<bool>,
        offset: Option<usize>,
        limit: Option<usize>,
        refresh: Option<bool>,
    ) -> Result<SessionPage, String> {
        if refresh.unwrap_or(false) {
            self.sessions().clear();
            self.lifecycles().clear();
        }
        let descriptors = self.filtered_descriptors(
            agent,
            query,
            project.as_ref(),
            include_subagents.unwrap_or(false),
        )?;
        let mut sessions: Vec<SessionSummary> =
            descriptors.into_iter().map(SessionSummary::from).collect();
        let total = sessions.len();
        let offset = offset.unwrap_or(0).min(total);
        let limit = limit
            .unwrap_or(DEFAULT_SESSION_PAGE_SIZE)
            .clamp(1, MAX_SESSION_PAGE_SIZE);
        let page_end = offset.saturating_add(limit).min(total);
        let has_more = page_end < total;
        sessions = sessions
            .into_iter()
            .skip(offset)
            .take(page_end.saturating_sub(offset))
            .collect();

        Ok(SessionPage {
            sessions,
            total,
            offset,
            has_more,
        })
    }

    fn search_memory(
        &self,
        agent: Option<String>,
        query: String,
        limit: Option<usize>,
    ) -> Result<Vec<MemoryHit>, String> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return Ok(Vec::new());
        }
        let parsed_agent = match agent.as_deref() {
            Some(value) => Some(parse_agent(value)?),
            None => None,
        };
        let filter = SessionFilter {
            agent: parsed_agent,
            project: None,
            since: None,
            limit: None,
        };
        let descriptors = match &self.archive {
            Some(archive) => self
                .app
                .list_sessions_with_archive(&filter, archive)
                .map_err(|error| error.to_string())?,
            None => self.app.list_sessions(&filter),
        };
        let cap = limit.unwrap_or(50).clamp(1, 200);
        let mut hits = Vec::new();
        for descriptor in descriptors {
            if hits.len() >= cap {
                break;
            }
            let Ok(body) = fs::read_to_string(&descriptor.path) else {
                continue;
            };
            let lowered = body.to_lowercase();
            for (index, (line, line_text)) in body.lines().zip(lowered.lines()).enumerate() {
                if !line_text.contains(&needle) {
                    continue;
                }
                let preview = line.chars().take(180).collect::<String>();
                let turn = self
                    .cached_session(descriptor.agent, descriptor.id.as_str(), false)
                    .ok()
                    .and_then(|cached| {
                        cached
                            .session
                            .events()
                            .iter()
                            .find(|event| event.source.line_no == (index + 1) as u32)
                            .and_then(|event| event.turn.map(|value| value.get()))
                    });
                hits.push(MemoryHit {
                    session_id: descriptor.id.to_string(),
                    agent: descriptor.agent.to_string(),
                    project: descriptor.project.clone(),
                    line: index + 1,
                    turn,
                    preview,
                });
                if hits.len() >= cap {
                    break;
                }
            }
        }
        Ok(hits)
    }

    fn inspect_session(&self, agent: AgentKind, id: &str) -> Result<SessionDetail, String> {
        let cached = self.cached_session(agent, id, false)?;
        let session = &cached.session;
        let growth = timeline(session);
        let peak_turn = session.peak_turn().map(|turn| turn.get());
        let metadata = session.metadata();
        let (model_usage, unattributed_model_turns) =
            model_usage(session, metadata.model.as_deref());

        Ok(SessionDetail {
            session: cached.descriptor.clone().into(),
            model: metadata.model.clone(),
            model_usage,
            unattributed_model_turns,
            agent_version: metadata.agent_version.clone(),
            git_branch: metadata.git_branch.clone(),
            turn_count: session.turn_count(),
            event_count: session.events().len(),
            total_output_tokens: session.total_output_tokens(),
            peak_turn,
            peak_prompt_tokens: session.peak_prompt_tokens(),
            context_window: metadata.context_window,
            fidelity: session.fidelity(),
            unrecognised_events: session.unrecognised_total(),
            unplaced_compactions: growth.unplaced_compactions,
            growth: growth
                .points
                .into_iter()
                .map(|point| GrowthPointSummary {
                    turn: point.turn,
                    prompt_tokens: point.prompt_tokens,
                    compaction: point.compaction.map(|event| CompactionSummary {
                        turn: Some(point.turn),
                        reclaimed: event.reclaimed,
                        line_no: event.line_no,
                    }),
                })
                .collect(),
            source: Some(SessionSourceSummary::from(&cached.source)),
        })
    }

    fn context(
        &self,
        agent: AgentKind,
        id: &str,
        turn: Option<u32>,
    ) -> Result<ContextDetail, String> {
        let cached = self.cached_session(agent, id, false)?;
        let turn = match turn {
            Some(turn) => TurnNumber::new(turn).map_err(|error| error.to_string())?,
            None => self
                .app
                .peak_turn(&cached.session)
                .ok_or_else(|| "this session has no turn with prompt usage".to_string())?,
        };
        let estimator = cached
            .chars_per_token()
            .map(ct_runtime::heuristic_estimator);
        let snapshot = match estimator.as_ref() {
            Some(estimator) => {
                self.app
                    .snapshot_with(&cached.session, cached.binding, turn, estimator)
            }
            None => self.app.snapshot(&cached.session, cached.binding, turn),
        }
        .map_err(|error| error.to_string())?;

        let contributors = snapshot
            .largest_contributors(20)
            .into_iter()
            .map(|item| ContributorSummary {
                id: item.id.to_string(),
                label: item.label,
                category: item.category.label().to_string(),
                source: format_source(&item.source),
                tokens: item.tokens,
                share: item.share,
                confidence: item.confidence,
            })
            .collect();

        // The full inventory, ranked and shared by the same domain call that
        // produces `contributors` above rather than by a second copy of the
        // share arithmetic here -- an unbounded limit is the whole difference.
        // Reimplementing it would put the denominator rule in two places, and
        // the one in the domain is the one with the honesty argument attached.
        let bodies: HashMap<&ContextItemId, &ContextItem> = snapshot
            .items()
            .iter()
            .map(|item| (&item.id, item))
            .collect();
        let items = snapshot
            .largest_contributors(usize::MAX)
            .into_iter()
            .map(|item| {
                let body = bodies.get(&item.id);
                ContextItemSummary {
                    id: item.id.to_string(),
                    label: item.label,
                    category: item.category.slug(),
                    source: format_source(&item.source),
                    tokens: item.tokens,
                    share: item.share,
                    confidence: item.confidence,
                    first_seen_turn: body.and_then(|body| body.first_seen_turn).map(|t| t.get()),
                    preview: body.and_then(|body| body.preview.clone()),
                }
            })
            .collect();

        Ok(ContextDetail {
            turn: turn.get(),
            model: snapshot.model().map(str::to_string),
            total_tokens: snapshot.total().tokens(),
            residual_tokens: snapshot.residual(),
            residual_is_meaningful: snapshot.residual_is_meaningful(),
            context_window: snapshot.context_window(),
            utilisation: snapshot.utilisation(),
            calibration_scale: snapshot.calibration_scale(),
            categories: snapshot
                .by_category()
                .into_iter()
                .map(CategorySummary::from)
                .collect(),
            contributors,
            items,
        })
    }

    fn doctor(
        &self,
        agent: AgentKind,
        id: &str,
        turn: Option<u32>,
    ) -> Result<DoctorReport, String> {
        let cached = self.cached_session(agent, id, true)?;
        let turn = match turn {
            Some(turn) => TurnNumber::new(turn).map_err(|error| error.to_string())?,
            None => self
                .app
                .peak_turn(&cached.session)
                .ok_or_else(|| "this session has no turn with prompt usage".to_string())?,
        };
        let estimator = cached
            .chars_per_token()
            .map(ct_runtime::heuristic_estimator);
        let snapshot = match estimator.as_ref() {
            Some(estimator) => {
                self.app
                    .snapshot_with(&cached.session, cached.binding, turn, estimator)
            }
            None => self.app.snapshot(&cached.session, cached.binding, turn),
        }
        .map_err(|error| error.to_string())?;

        // Neither section below can see an item with no content measurement --
        // that pass is what `analyzed: true` above requested. One count covers
        // both: they share the same gate, so a per-detector split would just
        // print the same number twice under two names. See CT-059.
        let unmeasured_items = unmeasured_content_items(snapshot.items());

        let duplicate_content = snapshot.duplicate_content();
        let duplicate_groups = duplicate_content.len();
        let repeated_tokens = duplicate_content.iter().fold(0u32, |total, group| {
            total.saturating_add(group.repeated_tokens)
        });
        let duplicates = duplicate_content
            .into_iter()
            .take(8)
            .map(|group| DuplicateSummary {
                copies: group.items.len(),
                total_tokens: group.total_tokens,
                repeated_tokens: group.repeated_tokens,
                share: group.share,
                confidence: group.confidence,
                items: group
                    .items
                    .into_iter()
                    .take(4)
                    .map(|item| DiagnosticItemSummary {
                        label: item.label,
                        source: format_source(&item.source),
                        tokens: item.tokens,
                    })
                    .collect(),
            })
            .collect();

        let low_entropy_content = snapshot.low_entropy_content();
        let low_entropy_items = low_entropy_content.len();
        let waste_score_tokens = low_entropy_content.iter().fold(0u32, |total, item| {
            total.saturating_add(item.waste_score_tokens)
        });
        let low_entropy = low_entropy_content
            .into_iter()
            .take(8)
            .map(|item| LowEntropySummary {
                label: item.label,
                source: format_source(&item.source),
                tokens: item.tokens,
                compression_ratio: item.compression_ratio,
                waste_score_tokens: item.waste_score_tokens,
                share: item.share,
                confidence: item.confidence,
            })
            .collect();

        // The raw-file scan is the slowest thing the desktop does. It runs on
        // the handle above with no cache lock held, so the session list stays
        // answerable while a large session is being read.
        let raw = ct_runtime::raw_event_source(&cached.descriptor.path);
        let secret_scan = self.app.scan_secrets(&cached.session, &raw);
        let secret_occurrences = secret_scan.occurrence_count();
        let secret_findings = secret_scan.findings.len();
        let secrets = secret_scan
            .findings
            .into_iter()
            .take(12)
            .map(|finding| SecretFindingSummary {
                kind: finding.kind.label().to_string(),
                occurrences: finding.occurrences,
                turn: finding.turn.map(|turn| turn.get()),
                line: finding.line_no,
                event_type: finding.event_type,
            })
            .collect();

        Ok(DoctorReport {
            turn: turn.get(),
            duplicate_groups,
            repeated_tokens,
            duplicates,
            low_entropy_items,
            waste_score_tokens,
            low_entropy,
            secret_findings,
            secret_occurrences,
            scanned_records: secret_scan.scanned_records,
            unreadable_records: secret_scan.unreadable_records,
            secrets,
            unmeasured_items,
        })
    }

    fn lifecycle(&self, agent: AgentKind, id: &str, item: &str) -> Result<LifecycleReport, String> {
        // The sweep walks every turn, so it too runs outside both locks; the
        // cache is only locked to read the handle and to publish the result.
        // The lookup is bound before the match: a guard in a match scrutinee
        // lives as long as the whole match, and re-locking inside an arm would
        // deadlock the command against itself.
        let key = (agent, id.to_string());
        let swept = self.lifecycles().get(&key);
        let sweep = match swept {
            Some(sweep) => sweep,
            None => {
                let cached = self.cached_session(agent, id, false)?;
                let sweep = Arc::new(self.app.sweep_lifecycles(&cached.session, cached.binding));
                self.lifecycles().insert(key, Arc::clone(&sweep));
                sweep
            }
        };

        let item_id = ContextItemId::new(item);
        let record = sweep.item(&item_id).ok_or_else(|| {
            "this contributor is no longer present after the session cache was refreshed"
                .to_string()
        })?;
        let life = sweep.lifecycle_of(record);
        let first_present = life.first_present();
        let last_present = life.last_present();
        let turns_present = life.turns_present();
        let first_seen_disagrees = life.first_seen_disagrees();
        let departure = life.departure.map(DepartureSummary::from);

        Ok(LifecycleReport {
            id: life.id.to_string(),
            label: life.label,
            category: life.category.label().to_string(),
            source: format_source(&life.source),
            first_present,
            last_present,
            turns_present,
            runs: life
                .runs
                .into_iter()
                .map(|run| TurnRunSummary {
                    from: run.from,
                    to: run.to,
                    turns: run.turns,
                })
                .collect(),
            departure,
            still_present: life.still_present,
            unknown_turns: life.unknown_turns,
            scanned_turns: life.scanned_turns,
            other_thread_turns: life.other_thread_turns,
            last_scanned_turn: life.last_scanned_turn,
            recorded_first_seen: life.recorded_first_seen,
            first_seen_disagrees,
        })
    }

    /// The structural autopsy of one compaction: what its replacement history
    /// dropped, preserved and added, looked up by the log line the marker on
    /// the growth chart carries.
    ///
    /// `PortError::Unsupported` is the agent-level refusal -- Claude Code
    /// never overrides `compaction_diffs`, so *every* one of its sessions
    /// hits this before any single compaction is even looked at. That is a
    /// different fact from [`CompactionDiffUnavailable`], which explains why
    /// one specific Codex compaction's history could not be read even though
    /// the agent generally supports this. Both are reported as typed,
    /// explained outcomes rather than the generic error banner, so a Claude
    /// Code user learns the evidence does not exist rather than that the
    /// feature is broken.
    fn compaction_diff(
        &self,
        agent: AgentKind,
        id: &str,
        line_no: u32,
    ) -> Result<CompactionDiffSummary, String> {
        let cached = self.cached_session(agent, id, false)?;
        let raw = ct_runtime::raw_event_source(&cached.descriptor.path);
        let diffs = match self
            .app
            .compaction_diffs(&cached.session, cached.binding, &raw)
        {
            Ok(diffs) => diffs,
            Err(AppError::Port(PortError::Unsupported(detail))) => {
                return Ok(CompactionDiffSummary::Unsupported { detail });
            }
            Err(error) => return Err(error.to_string()),
        };
        let diff = diffs
            .into_iter()
            .find(|diff| diff.source().line_no == line_no)
            .ok_or_else(|| format!("no compaction recorded at line {line_no} in this session"))?;
        Ok(CompactionDiffSummary::from(diff))
    }

    /// Summarise the whole local corpus, reusing the last sweep when nothing
    /// on disk has changed.
    ///
    /// The cache is keyed by a fingerprint of every discovered session --
    /// path, size and last activity -- rather than by a timestamp. Discovery
    /// is the cheap half of the sweep (a `stat` and a short read per file), so
    /// re-running it to decide whether the expensive half is still valid costs
    /// a fraction of what it saves, and it notices a session that changed
    /// without growing.
    fn corpus(&self, refresh: bool, app: &AppHandle) -> Result<CorpusSummary, String> {
        let fingerprint = self.corpus_fingerprint();
        if !refresh {
            if let Some(cached) = self
                .corpus
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .filter(|(seen, _)| *seen == fingerprint)
            {
                return Ok(CorpusSummary::from_cached(&cached.1, true));
            }
            // An in-memory cache dies with the process, so without this every
            // launch pays for the dashboard again -- and the dashboard is what
            // the app now opens on.
            if let Some(report) = read_corpus_cache(&fingerprint) {
                let summary = CorpusSummary::from_cached(&report, true);
                *self
                    .corpus
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((fingerprint, report));
                return Ok(summary);
            }
        }

        let report = self.app.sweep_corpus(|done, total| {
            let _ = app.emit(
                "contexttrace://corpus-progress",
                CorpusProgress { done, total },
            );
        });
        let summary = CorpusSummary::from_cached(&report, false);
        write_corpus_cache(&fingerprint, &report);
        *self
            .corpus
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some((fingerprint, report));
        Ok(summary)
    }

    /// The last sweep as it stands, without asking whether it still holds.
    ///
    /// A fingerprint is all-or-nothing: one session that grew by a line
    /// invalidates the whole report, and on a machine running agents that is
    /// most launches. So the dashboard paints these numbers first and lets a
    /// real sweep reconcile behind them. It is offered as last time's answer
    /// -- `cached` is true -- and never as a claim about now.
    fn corpus_remembered(&self) -> Option<CorpusSummary> {
        if let Some((_, report)) = self
            .corpus
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
        {
            return Some(CorpusSummary::from_cached(report, true));
        }
        read_corpus_cache_file().map(|cached| CorpusSummary::from_cached(&cached.report, true))
    }

    /// What the corpus looked like when a sweep ran.
    fn corpus_fingerprint(&self) -> Vec<ct_domain::SessionFingerprint> {
        let mut fingerprints: Vec<_> = self
            .app
            .list_sessions(&SessionFilter {
                agent: None,
                project: None,
                since: None,
                limit: None,
            })
            .into_iter()
            .map(|descriptor| ct_domain::SessionFingerprint {
                path: descriptor.path,
                size_bytes: descriptor.size_bytes,
                last_activity: descriptor.last_activity,
            })
            .collect();
        fingerprints.sort_by(|left, right| left.path.cmp(&right.path));
        fingerprints
    }

    /// One window of the session's conversation.
    ///
    /// Paged rather than whole for the same reason the catalog is: the largest
    /// local session is 6.8 MB, and reading a conversation must not mean
    /// materialising one.
    fn transcript(
        &self,
        agent: AgentKind,
        id: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<TranscriptPageSummary, String> {
        let cached = self.cached_session(agent, id, false)?;
        let raw = ct_runtime::raw_event_source(&cached.descriptor.path);
        let limit = limit
            .unwrap_or(DEFAULT_TRANSCRIPT_PAGE_SIZE)
            .clamp(1, MAX_TRANSCRIPT_PAGE_SIZE);
        let page = self.app.transcript(
            &cached.session,
            cached.binding,
            &raw,
            offset.unwrap_or(0),
            limit,
        );
        Ok(TranscriptPageSummary::from(page))
    }

    /// One transcript entry in full, for an entry the reader expanded.
    fn transcript_entry(
        &self,
        agent: AgentKind,
        id: &str,
        index: usize,
    ) -> Result<TranscriptEntrySummary, String> {
        let cached = self.cached_session(agent, id, false)?;
        let raw = ct_runtime::raw_event_source(&cached.descriptor.path);
        self.app
            .transcript_entry(&cached.session, cached.binding, &raw, index)
            .map(TranscriptEntrySummary::from)
            .ok_or_else(|| format!("this session's transcript has no entry {index}"))
    }

    /// Compare two turns, which may belong to two different sessions.
    ///
    /// Same-session comparisons -- the only ones reachable before this
    /// widening -- stay cheap: `cached_session` is asked for the right side
    /// only when it names a genuinely different session, so the common case
    /// (one session, two turns) still fits its characters-per-token ratio
    /// once rather than twice. Fitting is a sweep of every turn, seconds on a
    /// long session, so paying for it twice on the commonest comparison would
    /// be a regression this widening must not introduce. Mirrors the
    /// optimisation `ct diff` already makes at `ct-cli/src/main.rs`, around
    /// the `Command::Diff` arm.
    ///
    /// Two sessions can be sized by two different instruments -- two Claude
    /// Code sessions fitted to different ratios, or a Codex session measured
    /// by a real tokenizer against a Claude Code one measured by a heuristic.
    /// `Comparability` is the domain's judgement of whether the resulting
    /// deltas may be read at all, and it is carried across the boundary
    /// rather than assumed away, exactly as it already is for the
    /// same-session case: this is the first desktop path where its `skewed`
    /// and `incomparable` arms are reachable rather than dead code.
    fn turn_diff(
        &self,
        left_agent: AgentKind,
        left_id: &str,
        left_turn: u32,
        right_agent: AgentKind,
        right_id: &str,
        right_turn: u32,
    ) -> Result<TurnDiffSummary, String> {
        let left_turn = TurnNumber::new(left_turn).map_err(|error| error.to_string())?;
        let right_turn = TurnNumber::new(right_turn).map_err(|error| error.to_string())?;

        let left_cached = self.cached_session(left_agent, left_id, false)?;
        let same_session = right_agent == left_agent && right_id == left_id;
        let right_cached = if same_session {
            Arc::clone(&left_cached)
        } else {
            self.cached_session(right_agent, right_id, false)?
        };

        let left_fitted = left_cached
            .chars_per_token()
            .map(ct_runtime::heuristic_estimator);
        let left_estimator: &dyn TokenEstimator = match left_fitted.as_ref() {
            Some(estimator) => estimator,
            None => self.app.binding_estimator(left_cached.binding),
        };
        let right_fitted = right_cached
            .chars_per_token()
            .map(ct_runtime::heuristic_estimator);
        let right_estimator: &dyn TokenEstimator = match right_fitted.as_ref() {
            Some(estimator) => estimator,
            None => self.app.binding_estimator(right_cached.binding),
        };

        let left_snapshot = self
            .app
            .snapshot_with(
                &left_cached.session,
                left_cached.binding,
                left_turn,
                left_estimator,
            )
            .map_err(|error| error.to_string())?;
        let right_snapshot = self
            .app
            .snapshot_with(
                &right_cached.session,
                right_cached.binding,
                right_turn,
                right_estimator,
            )
            .map_err(|error| error.to_string())?;

        let left_instrument = ct_application::Instrument::new(
            left_estimator.name(),
            left_estimator.chars_per_token(),
        );
        let right_instrument = ct_application::Instrument::new(
            right_estimator.name(),
            right_estimator.chars_per_token(),
        );
        let diff = ct_application::compare(
            ct_application::Side {
                snapshot: &left_snapshot,
                instrument: left_instrument,
            },
            ct_application::Side {
                snapshot: &right_snapshot,
                instrument: right_instrument,
            },
        );
        Ok(TurnDiffSummary::from(diff))
    }

    fn instruction_files(
        &self,
        agent: AgentKind,
        id: &str,
    ) -> Result<InstructionFileReportSummary, String> {
        let cached = self.cached_session(agent, id, true)?;
        let hasher = ct_runtime::content_hasher();
        Ok(InstructionFileReportSummary::from(
            ct_application::compare_instruction_files(&cached.session, &hasher),
        ))
    }

    fn cost(
        &self,
        agent: AgentKind,
        id: &str,
        pricing_path: Option<&str>,
        forecast_turns: Option<u32>,
    ) -> Result<CostReportSummary, String> {
        let cached = self.cached_session(agent, id, false)?;
        let pricing = pricing_path
            .map(ct_application::PricingOverrides::from_path)
            .transpose()?;
        Ok(CostReportSummary::from(
            ct_application::project_cost_scenario(
                &cached.session,
                &ct_application::CostScenario {
                    forecast_turns,
                    pricing,
                    ..Default::default()
                },
            ),
        ))
    }

    fn temporal_ghost(
        &self,
        agent: AgentKind,
        id: &str,
        left_turn: u32,
        right_turn: u32,
    ) -> Result<TemporalGhostSummary, String> {
        let left_turn = TurnNumber::new(left_turn).map_err(|error| error.to_string())?;
        let right_turn = TurnNumber::new(right_turn).map_err(|error| error.to_string())?;
        let cached = self.cached_session(agent, id, false)?;
        let fitted = cached
            .chars_per_token()
            .map(ct_runtime::heuristic_estimator);
        let estimator: &dyn TokenEstimator = match fitted.as_ref() {
            Some(estimator) => estimator,
            None => self.app.binding_estimator(cached.binding),
        };
        let left = self
            .app
            .snapshot_with(&cached.session, cached.binding, left_turn, estimator)
            .map_err(|error| error.to_string())?;
        let right = self
            .app
            .snapshot_with(&cached.session, cached.binding, right_turn, estimator)
            .map_err(|error| error.to_string())?;
        let instrument =
            ct_application::Instrument::new(estimator.name(), estimator.chars_per_token());
        Ok(TemporalGhostSummary::from(ct_application::temporal_ghost(
            &left,
            instrument.clone(),
            &right,
            instrument,
        )))
    }

    /// The unlogged remainder across a session's turns: CT-072's signature
    /// measurement, previously reachable only from `ct residual`.
    ///
    /// A sum type, not a `DerivedRatio` with nullable fields, because there
    /// are four genuinely different situations here and a nullable field
    /// cannot say which one a caller has without re-deriving it. The agent
    /// refusal (`agentNotFitted`) and the growth refusal (`insufficientGrowth`)
    /// are both cases where no ratio exists to draw from at all. The harder
    /// one is `overCounted`: `derive_ratio` can return `Some` for a session
    /// whose reconstruction over-counts, with `unlogged_overhead: None` --
    /// and every per-turn remainder in its series is `None` too, because
    /// [`ResidualPoint::unlogged`] and
    /// [`ct_application::SessionRatio::unlogged_overhead`] share the same
    /// "reconstructed more than the prompt held" refusal.
    /// Routing that session into `fitted` would render a confident header
    /// (a chars-per-token figure, a dispersion, a step threshold) above a
    /// chart with nothing plottable on it -- CT-072's "drawing a line through
    /// nothing" made literal. So a series is only ever reported as `fitted`
    /// when at least one turn has a known remainder; otherwise, fitted or
    /// not, it is `overCounted`. That discrimination is
    /// [`series_has_a_measurable_remainder`], kept as a pure function over
    /// `&[ResidualPoint]` so it is checked once and tested without needing a
    /// session or a ratio derivation to exercise it.
    ///
    /// Order of checks matters. Codex is refused before `derive_ratio` is
    /// even consulted -- the refusal is about the agent never being fitted at
    /// all, not about this particular session's growth, and asking the
    /// question in the other order would describe a structural refusal as if
    /// it were a data problem.
    fn residual(&self, agent: AgentKind, id: &str) -> Result<ResidualReport, String> {
        if agent == AgentKind::Codex {
            return Ok(ResidualReport::AgentNotFitted {
                agent: agent.to_string(),
            });
        }

        let cached = self.cached_session(agent, id, false)?;

        let Some(ratio) = cached.ratio else {
            let turns_with_usage = cached
                .session
                .turns()
                .iter()
                .filter(|turn| turn.prompt_tokens().is_some())
                .count();
            return Ok(ResidualReport::InsufficientGrowth { turns_with_usage });
        };

        let series = self
            .app
            .residual_series(&cached.session, cached.binding, ratio);

        if !series_has_a_measurable_remainder(&series) {
            return Ok(ResidualReport::OverCounted {
                chars_per_token: ratio.chars_per_token,
                pairs_used: ratio.pairs_used,
                dispersion: ratio.dispersion,
                turns_measured: series.len(),
            });
        }

        let compaction_turns: Vec<u32> = cached
            .session
            .compactions()
            .iter()
            .filter_map(|(_, event)| event.turn.map(|turn| turn.get()))
            .collect();

        let over_counted_turns = series
            .iter()
            .filter(|point| point.unlogged.is_none())
            .count();
        let steps = residual_steps(&series)
            .into_iter()
            .map(|step| {
                // Mirrors `ct residual`'s own rule (`ct-cli/src/render.rs`): a
                // compaction rewrites the whole prompt, so a step next to one
                // already has a cause on record. Attributing it to an
                // unrecorded harness change too would invent a second
                // explanation for something the session already accounts for.
                let near_compaction = compaction_turns
                    .iter()
                    .any(|turn| turn.abs_diff(step.turn) <= STEP_ATTRIBUTION_WINDOW);
                ResidualStepSummary {
                    turn: step.turn,
                    from: step.from,
                    to: step.to,
                    growth: step.growth(),
                    near_compaction,
                }
            })
            .collect();

        Ok(ResidualReport::Fitted {
            chars_per_token: ratio.chars_per_token,
            pairs_used: ratio.pairs_used,
            dispersion: ratio.dispersion,
            unlogged_overhead: ratio.unlogged_overhead,
            turns_measured: series.len(),
            over_counted_turns,
            step_threshold: RESIDUAL_STEP_THRESHOLD,
            prompt_confidence: Confidence::Observed,
            remainder_confidence: Confidence::Derived,
            points: series.iter().map(ResidualPointSummary::from).collect(),
            steps,
        })
    }

    /// Every session held in the archive, most recently archived first.
    ///
    /// `store.root()` is read here rather than left for a caller to derive
    /// separately from `startup()`: the two must always name the same
    /// directory, and reading it once from the one seam that constructs the
    /// store (`ct_runtime::archive_store`) is what keeps that true rather
    /// than merely intended.
    fn archived_sessions(&self) -> Result<ArchiveHolding, String> {
        let store = ct_runtime::archive_store();
        let entries = self
            .app
            .archived_sessions(&store)
            .map_err(|error| error.to_string())?;
        Ok(ArchiveHolding {
            root: store.root(),
            entries: entries.into_iter().map(ArchiveEntrySummary::from).collect(),
        })
    }

    /// Copy one session into the archive, named by both halves of its
    /// identity.
    ///
    /// Always `archive_session_in_agent`, never the unscoped
    /// `ContextTrace::archive_session`: the desktop always has the agent a
    /// catalog row came from on hand, and two agents can hold the same id.
    /// The unscoped lookup resolves to whichever binding was registered
    /// first, which would make this silently archive the wrong session on a
    /// collision instead of the one actually named.
    fn archive_session(
        &self,
        agent: AgentKind,
        id: &str,
        raw: bool,
    ) -> Result<ArchiveEntrySummary, String> {
        let store = ct_runtime::archive_store();
        let mode = if raw {
            RedactionMode::Raw
        } else {
            RedactionMode::Redacted
        };
        let entry = self
            .app
            .archive_session_in_agent(agent, id, &store, mode)
            .map_err(|error| error.to_string())?;
        Ok(ArchiveEntrySummary::from(entry))
    }

    /// Re-digest one archived session, named by both halves of its identity.
    ///
    /// `verify_archived_in_agent` for the reason `archive_session` above
    /// gives, and for a sharper one here: verification is the feature that
    /// exists for a session whose source log may already be gone, so it
    /// cannot fall back to resolving against the live corpus at all. Scoping
    /// by agent is the only way the second of two colliding ids is reachable.
    fn verify_archived(&self, agent: AgentKind, id: &str) -> Result<ArchiveVerification, String> {
        let store = ct_runtime::archive_store();
        let integrity = self
            .app
            .verify_archived_in_agent(agent, id, &store)
            .map_err(|error| error.to_string())?;
        Ok(ArchiveVerification::from(integrity))
    }

    /// Stream one session out as NDJSON, and report what was written.
    ///
    /// The estimator is the session's own fitted ratio where one exists, which
    /// is the same choice every other view here makes. Reaching for the
    /// binding's default instead would make an export disagree with the
    /// context panel beside it about the size of the same turn, with nothing
    /// on either side to explain the difference.
    fn export_session(
        &self,
        agent: AgentKind,
        id: &str,
        redact_secrets: bool,
    ) -> Result<ExportOutcome, String> {
        self.export_session_into(
            &ct_runtime::export_dir(&ct_runtime::archive_store().root()),
            agent,
            id,
            redact_secrets,
        )
    }

    /// The half of [`AppState::export_session`] that does not decide *where*.
    ///
    /// Split out so tests can name a scratch directory. The alternative --
    /// pointing `CONTEXTTRACE_ARCHIVE` at one -- is process-wide state that
    /// parallel tests would race on, and getting it wrong means writing into
    /// the archive of whoever is running the suite.
    fn export_session_into(
        &self,
        root: &Path,
        agent: AgentKind,
        id: &str,
        redact_secrets: bool,
    ) -> Result<ExportOutcome, String> {
        let cached = self.cached_session(agent, id, false)?;
        let fitted = cached
            .chars_per_token()
            .map(ct_runtime::heuristic_estimator);
        let estimator: &dyn TokenEstimator = match fitted.as_ref() {
            Some(estimator) => estimator,
            None => self.app.binding_estimator(cached.binding),
        };
        let redaction = if redact_secrets {
            ExportRedaction::Secrets
        } else {
            ExportRedaction::None
        };

        // The same rule the archive names its copies by, for the same reason:
        // an id is an opaque string, and one containing a separator would
        // place this file outside the directory chosen for it.
        let stem = cached.descriptor.id.file_stem().ok_or_else(|| {
            format!(
                "session id '{}' is not safe to use as a filename",
                cached.descriptor.id
            )
        })?;
        let dir = root.join(cached.descriptor.agent.label());
        fs::create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let path = dir.join(format!("{stem}.ndjson"));

        // Written beside the destination and renamed over it only once the
        // last record is on disk. A reader who finds the file finds a whole
        // export: an interrupted one leaves the previous copy standing rather
        // than a truncated file that still parses line by line and quietly
        // stops half a session early.
        let pending = path.with_extension("ndjson.pending");
        let outcome = self.write_export(&pending, &cached, estimator, redaction);
        let outcome = match outcome {
            Ok(outcome) => outcome,
            Err(error) => {
                let _ = fs::remove_file(&pending);
                return Err(error);
            }
        };
        fs::rename(&pending, &path).map_err(|error| format!("{}: {error}", path.display()))?;

        Ok(ExportOutcome {
            path: path.display().to_string(),
            bytes: outcome.bytes,
            records: outcome.records,
            redaction: redaction_label(redaction).to_string(),
            redactions: outcome.redactions,
        })
    }

    /// The streaming half of [`AppState::export_session`], kept separate so
    /// the caller can delete a half-written file on any failure path.
    ///
    /// One record is serialized at a time and written straight through a
    /// buffer. Collecting the export into a `String` first would materialise a
    /// session that can reach tens of megabytes -- the one thing every other
    /// read path in this tool is careful not to do.
    fn write_export(
        &self,
        pending: &Path,
        cached: &CachedSession,
        estimator: &dyn TokenEstimator,
        redaction: ExportRedaction,
    ) -> Result<WrittenExport, String> {
        let file =
            fs::File::create(pending).map_err(|error| format!("{}: {error}", pending.display()))?;
        let mut writer = BufWriter::new(file);
        let mut records = 0u64;
        let mut bytes = 0u64;

        let report = self
            .app
            .export_ndjson(
                &cached.session,
                &cached.descriptor.path,
                cached.binding,
                estimator,
                redaction,
                |record| {
                    let line = serde_json::to_string(record)
                        .map_err(|error| AppError::Port(PortError::Io(error.to_string())))?;
                    writer
                        .write_all(line.as_bytes())
                        .and_then(|()| writer.write_all(b"\n"))
                        .map_err(|error| AppError::Port(PortError::Io(error.to_string())))?;
                    records += 1;
                    bytes += line.len() as u64 + 1;
                    Ok(())
                },
            )
            .map_err(|error| error.to_string())?;

        writer
            .flush()
            .map_err(|error| format!("{}: {error}", pending.display()))?;
        Ok(WrittenExport {
            records,
            bytes,
            redactions: report.redactions,
        })
    }
}

/// What [`AppState::write_export`] observed while streaming, before the file
/// it wrote has been renamed into place and can be described as an export.
struct WrittenExport {
    records: u64,
    bytes: u64,
    redactions: usize,
}

/// The wire name for what an export was asked to do about credentials.
///
/// Spelled out here rather than derived from the enum's `Debug`: this string
/// is a claim about whether the file on disk still holds credentials, and it
/// should not change because someone renamed a variant.
fn redaction_label(redaction: ExportRedaction) -> &'static str {
    match redaction {
        ExportRedaction::None => "none",
        ExportRedaction::Secrets => "secrets",
    }
}

/// Whether at least one turn in a residual series has a known remainder.
///
/// The one judgement call CT-072 forbids getting wrong: a session can fit a
/// ratio and still have nothing plottable, when reconstruction over-counts on
/// every turn that was measured. An empty series answers `false` here too --
/// nothing can be drawn from zero points either -- so the caller does not
/// need a separate emptiness check before asking this one.
fn series_has_a_measurable_remainder(series: &[ResidualPoint]) -> bool {
    series.iter().any(|point| point.unlogged.is_some())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootSummary {
    agent: String,
    paths: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupSummary {
    roots: Vec<RootSummary>,
    warnings: Vec<String>,
    /// The one directory `archive_session` and `export_session` write to --
    /// the write-side counterpart of `roots`. Named here for the same reason
    /// `ct roots` names it out loud: a tool whose privacy claim rests on
    /// being auditable has to state every local path it touches, not just
    /// the ones it reads.
    archive_root: String,
}

/// Mirrors [`Comparability`], which decides whether the token deltas beside it
/// may be read as findings at all.
///
/// A sum type on both sides of the boundary, for the reason the domain gives:
/// these are not degrees of one claim. `identical` says the subtraction is
/// exact, `skewed` says it is exact to within a stated bound, and
/// `incomparable` says no bound exists — and the third is not "skew zero", it
/// is the absence of a scale relating the two sides. Flattening this to a
/// nullable number would make the strongest claim available (`0`) the value a
/// missing one falls back to.
///
/// `rename_all` is repeated per variant deliberately: on an enum it renames
/// only the variant tag, never each struct-variant's own fields.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ComparabilitySummary {
    #[serde(rename_all = "camelCase")]
    Identical { estimator: String },
    #[serde(rename_all = "camelCase")]
    Skewed {
        left: String,
        right: String,
        skew: f32,
    },
    #[serde(rename_all = "camelCase")]
    Incomparable {
        left: String,
        right: String,
        reason: String,
    },
}

impl From<Comparability> for ComparabilitySummary {
    fn from(value: Comparability) -> Self {
        match value {
            Comparability::Identical { estimator } => Self::Identical { estimator },
            Comparability::Skewed { left, right, skew } => Self::Skewed { left, right, skew },
            Comparability::Incomparable {
                left,
                right,
                reason,
            } => Self::Incomparable {
                left,
                right,
                reason,
            },
        }
    }
}

/// One side of a turn comparison, labelled by the session it came from.
///
/// `id` and `agent` exist because the two sides may now name two different
/// sessions: a comparison view that only showed `turn` could not tell a
/// reader which session a number belonged to once a cross-session diff is
/// possible.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSideSummary {
    id: String,
    agent: String,
    turn: u32,
    total_tokens: u32,
    items: usize,
    residual: u32,
    confidence: Confidence,
}

/// One category's figures on both sides.
///
/// `instrument_bound` is `None` when no bound can be stated, and `meaningful`
/// is then false: the delta is arithmetic with no claim attached. Both travel,
/// so the frontend never has to re-derive the judgement from the number.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryDeltaSummary {
    category: String,
    left: u32,
    right: u32,
    delta: i64,
    left_items: usize,
    right_items: usize,
    /// Change in item count. A count, so neither instrument can distort it --
    /// which makes it the row's honest fallback when `meaningful` is false.
    item_delta: i64,
    instrument_bound: Option<u32>,
    meaningful: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDeltaSummary {
    tool: String,
    left_calls: usize,
    right_calls: usize,
    left_tokens: u32,
    right_tokens: u32,
    call_delta: i64,
    token_delta: i64,
    instrument_bound: Option<u32>,
    meaningful: bool,
}

/// The comparison, as the desktop reads it.
///
/// `prompt_delta` is carried separately from every category row because it is
/// the one figure free of the instrument question: both sides are read out of
/// the sessions' own usage records. `totals_are_observed` says whether that
/// held, so a view can lead with the number that needs no caveat only when it
/// has one.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnDiffSummary {
    left: TurnSideSummary,
    right: TurnSideSummary,
    comparability: ComparabilitySummary,
    prompt_delta: i64,
    totals_are_observed: bool,
    categories: Vec<CategoryDeltaSummary>,
    tools: Vec<ToolDeltaSummary>,
}

impl From<SessionDiff> for TurnDiffSummary {
    fn from(diff: SessionDiff) -> Self {
        let prompt_delta = diff.prompt_delta();
        let totals_are_observed = diff.totals_are_observed();
        let side = |summary: &ct_application::SideSummary| TurnSideSummary {
            id: summary.session_id.clone(),
            agent: summary.agent.to_string(),
            turn: summary.turn,
            total_tokens: summary.total.tokens(),
            items: summary.items,
            residual: summary.residual,
            confidence: summary.total.confidence(),
        };
        Self {
            left: side(&diff.left),
            right: side(&diff.right),
            prompt_delta,
            totals_are_observed,
            categories: diff
                .categories
                .iter()
                .map(|row| CategoryDeltaSummary {
                    category: row.category.label().to_string(),
                    left: row.left,
                    right: row.right,
                    delta: row.delta,
                    left_items: row.left_items,
                    right_items: row.right_items,
                    item_delta: row.item_delta(),
                    instrument_bound: row.instrument_bound,
                    meaningful: row.is_meaningful(),
                })
                .collect(),
            tools: diff
                .tools
                .iter()
                .map(|row| ToolDeltaSummary {
                    tool: row.tool.clone(),
                    left_calls: row.left_calls,
                    right_calls: row.right_calls,
                    left_tokens: row.left_tokens,
                    right_tokens: row.right_tokens,
                    call_delta: row.call_delta(),
                    token_delta: row.token_delta(),
                    instrument_bound: row.instrument_bound,
                    meaningful: row.tokens_are_meaningful(),
                })
                .collect(),
            comparability: ComparabilitySummary::from(diff.comparability),
        }
    }
}

/// One turn's account of its own prompt, as the desktop reads it.
///
/// Mirrors [`ResidualPoint`]. `unlogged` stays `null` rather than `0` when
/// reconstruction over-counted the turn -- see [`ResidualReport`] for why a
/// session that never leaves this `null` cannot be reported as `fitted` at
/// all, and why a turn that does is still shown here rather than dropped.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidualPointSummary {
    turn: u32,
    prompt_tokens: u32,
    accounted: u32,
    unlogged: Option<u32>,
    items: usize,
}

impl From<&ResidualPoint> for ResidualPointSummary {
    fn from(value: &ResidualPoint) -> Self {
        Self {
            turn: value.turn,
            prompt_tokens: value.prompt_tokens,
            accounted: value.accounted,
            unlogged: value.unlogged,
            items: value.items,
        }
    }
}

/// One sustained change in the unlogged remainder, as the desktop reads it.
///
/// `near_compaction` is computed here rather than left for the frontend to
/// infer, for the same reason `ct residual` computes it in Rust rather than
/// printing raw compaction turns beside the step list: "close enough to a
/// compaction to already be explained" is a judgement this backend already
/// makes once, on the CLI path, and a second implementation in TypeScript
/// would be a second place for that threshold to drift from
/// [`STEP_ATTRIBUTION_WINDOW`].
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidualStepSummary {
    turn: u32,
    from: u32,
    to: u32,
    growth: i64,
    near_compaction: bool,
}

/// A session's unlogged remainder, across its turns.
///
/// A sum type rather than a `DerivedRatio` DTO with nullable fields --
/// [`AppState::residual`] documents why in full, but the shape of the
/// argument is that this crosses the exact boundary CT-072 is about:
/// "the panel must render \[a refused fit\] as the answer, not fall back to a
/// plausible-looking curve." A nullable `unloggedOverhead` on a single struct
/// would let a caller build that plausible-looking curve out of `points` even
/// when the fit backing it does not exist; a caller matching on `kind` cannot,
/// because `points` only exists on the `fitted` variant.
///
/// `rename_all` is repeated on every variant deliberately: on an enum it
/// renames only the `kind` tag, never a struct variant's own fields -- see
/// `ComparabilitySummary` above, which this file has been bitten by before.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ResidualReport {
    #[serde(rename_all = "camelCase")]
    Fitted {
        chars_per_token: f32,
        pairs_used: u32,
        dispersion: f32,
        /// The session's typical unlogged constant. `null` when the fit's
        /// overhead came out negative -- some turns are still plottable
        /// (`points` is never empty here), but the session-wide figure this
        /// view would otherwise headline is itself a refusal.
        unlogged_overhead: Option<u32>,
        turns_measured: usize,
        /// Points in `points` whose own remainder is unknown -- a stated
        /// figure, not a silently shorter chart.
        over_counted_turns: usize,
        step_threshold: i64,
        prompt_confidence: Confidence,
        remainder_confidence: Confidence,
        points: Vec<ResidualPointSummary>,
        steps: Vec<ResidualStepSummary>,
    },
    /// A ratio fitted, but no turn in the series yields a positive remainder.
    /// See [`AppState::residual`] for why this is not folded into `fitted`
    /// with an empty `points`.
    #[serde(rename_all = "camelCase")]
    OverCounted {
        chars_per_token: f32,
        pairs_used: u32,
        dispersion: f32,
        turns_measured: usize,
    },
    /// This view is built on a fitted ratio, and this agent is never fitted
    /// one at all -- see `ct_runtime::calibrate_session`, which only ever
    /// attempts a fit for Claude Code.
    #[serde(rename_all = "camelCase")]
    AgentNotFitted { agent: String },
    /// Claude Code, but the session lacks enough turn-to-turn growth for
    /// `derive_ratio` to fit anything.
    #[serde(rename_all = "camelCase")]
    InsufficientGrowth { turns_with_usage: usize },
}

/// `kind` is `"subagent"` if and only if `parent` is present, because the
/// domain type makes the other combination unrepresentable and this DTO must
/// not reopen that door on the way to JSON.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadRoleSummary {
    kind: &'static str,
    parent: Option<String>,
}

impl From<ThreadRole> for ThreadRoleSummary {
    fn from(value: ThreadRole) -> Self {
        match value {
            ThreadRole::Root => Self {
                kind: "root",
                parent: None,
            },
            ThreadRole::Subagent { parent } => Self {
                kind: "subagent",
                parent: Some(parent.to_string()),
            },
        }
    }
}

/// Which project a session listing is narrowed to.
///
/// Three states, because `Option<String>` can only express two and the third
/// is real: a Codex rollout whose log never recorded a `cwd` has no folder to
/// name, and "every project" must not be confused with "the ones with none".
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectFilter {
    Unrecorded,
    Path(String),
}

/// One selectable project, with the number of sessions selecting it would show.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    /// The full path, which is what the filter matches on. `None` is the entry
    /// for sessions whose log recorded no folder.
    pub path: Option<String>,
    /// The leaf name, for display.
    pub label: String,
    pub count: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    id: String,
    agent: String,
    path: String,
    size_bytes: u64,
    project: Option<String>,
    /// A recognisable name for the session, with the provenance of that name
    /// beside it -- an agent's own title and a first prompt are different
    /// claims and the row says which it is showing.
    title: Option<SessionTitleSummary>,
    git_branch: Option<String>,
    started_at: Option<String>,
    last_activity: Option<String>,
    thread_role: ThreadRoleSummary,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTitleSummary {
    text: String,
    source: &'static str,
}

impl From<ct_domain::SessionTitle> for SessionTitleSummary {
    fn from(value: ct_domain::SessionTitle) -> Self {
        Self {
            text: value.text,
            source: match value.source {
                ct_domain::TitleSource::AgentGenerated => "agentGenerated",
                ct_domain::TitleSource::FirstPrompt => "firstPrompt",
            },
        }
    }
}

/// A bounded, searchable page of locally discovered sessions.
///
/// `total` is the number of matches before paging, so a desktop client can
/// never mistake a first page for the entire catalog.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionPage {
    sessions: Vec<SessionSummary>,
    total: usize,
    offset: usize,
    has_more: bool,
}

/// A bounded local content hit. The preview is a short raw-line excerpt; it
/// is never sent over a network and the search never mutates the log.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryHit {
    session_id: String,
    agent: String,
    project: Option<String>,
    line: usize,
    turn: Option<u32>,
    preview: String,
}

impl From<SessionDescriptor> for SessionSummary {
    fn from(value: SessionDescriptor) -> Self {
        Self {
            id: value.id.to_string(),
            agent: value.agent.to_string(),
            path: value.path,
            size_bytes: value.size_bytes,
            project: value.project,
            title: value.title.map(SessionTitleSummary::from),
            git_branch: value.git_branch,
            started_at: value.started_at.map(|time| time.to_rfc3339()),
            last_activity: value.last_activity.map(|time| time.to_rfc3339()),
            thread_role: value.thread_role.into(),
        }
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct CorpusProgress {
    done: usize,
    total: usize,
}

/// The corpus sweep, plus whether this answer came from the cache.
///
/// `cached` is presented rather than hidden because the two are different
/// claims about freshness, and a summary of a corpus that changed since it was
/// computed should say so rather than look identical to one that did not.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusSummary {
    #[serde(flatten)]
    report: ct_application::CorpusReport,
    cached: bool,
}

impl CorpusSummary {
    fn from_cached(report: &ct_application::CorpusReport, cached: bool) -> Self {
        Self {
            report: report.clone(),
            cached,
        }
    }
}

/// One entry of a session's conversation, as the desktop renders it.
///
/// `chars` is the entry's whole length and `text` may be a truncated prefix of
/// it, which is the pair that lets a collapsed row state its weight honestly.
/// The two must not be confused: a row showing 2,000 characters of a 38,000
/// character tool result is the case this view exists to make visible.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptEntrySummary {
    index: usize,
    kind: &'static str,
    turn: Option<u32>,
    label: Option<String>,
    text: String,
    truncated: bool,
    chars: Option<u32>,
    sidechain: bool,
    error: bool,
    line: u32,
    /// Whether this kind of entry is machinery a reader scrolls past rather
    /// than reads. Decided in the application layer so the CLI and the desktop
    /// cannot disagree about what a conversation looks like.
    collapsed: bool,
}

impl From<ct_application::TranscriptEntry> for TranscriptEntrySummary {
    fn from(value: ct_application::TranscriptEntry) -> Self {
        Self {
            index: value.index,
            kind: transcript_kind_label(value.kind),
            turn: value.turn,
            label: value.label,
            text: value.text,
            truncated: value.truncated,
            chars: value.chars,
            sidechain: value.sidechain,
            error: value.error,
            line: value.line,
            collapsed: value.kind.collapsed_by_default(),
        }
    }
}

fn transcript_kind_label(kind: ct_application::TranscriptKind) -> &'static str {
    match kind {
        ct_application::TranscriptKind::User => "user",
        ct_application::TranscriptKind::Assistant => "assistant",
        ct_application::TranscriptKind::Reasoning => "reasoning",
        ct_application::TranscriptKind::ToolCall => "toolCall",
        ct_application::TranscriptKind::ToolResult => "toolResult",
        ct_application::TranscriptKind::Injection => "injection",
        ct_application::TranscriptKind::Compaction => "compaction",
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPageSummary {
    entries: Vec<TranscriptEntrySummary>,
    total: usize,
    offset: usize,
    has_more: bool,
}

impl From<ct_application::TranscriptPage> for TranscriptPageSummary {
    fn from(value: ct_application::TranscriptPage) -> Self {
        Self {
            entries: value
                .entries
                .into_iter()
                .map(TranscriptEntrySummary::from)
                .collect(),
            total: value.total,
            offset: value.offset,
            has_more: value.has_more,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummary {
    turn: Option<u32>,
    reclaimed: Option<u32>,
    /// The log line this compaction was recorded on -- the identity
    /// [`AppState::compaction_diff`] looks its autopsy up by. A turn number
    /// is not always present and, per `growth::timeline`'s turn-keyed map,
    /// not guaranteed unique across compactions; the line is.
    line_no: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrowthPointSummary {
    turn: u32,
    prompt_tokens: Option<u32>,
    compaction: Option<CompactionSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionDetail {
    session: SessionSummary,
    model: Option<String>,
    model_usage: Vec<ModelUsageSummary>,
    unattributed_model_turns: usize,
    agent_version: Option<String>,
    git_branch: Option<String>,
    turn_count: usize,
    event_count: usize,
    total_output_tokens: u32,
    peak_turn: Option<u32>,
    peak_prompt_tokens: Option<u32>,
    context_window: Option<u32>,
    fidelity: f32,
    unrecognised_events: u32,
    unplaced_compactions: usize,
    growth: Vec<GrowthPointSummary>,
    source: Option<SessionSourceSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageSummary {
    model: String,
    turns: usize,
}

/// Count models on completed turns, keeping missing per-turn model records
/// visible instead of assigning them the session metadata model. The metadata
/// fallback only keeps a known session model visible when no turn recorded one;
/// its zero count makes clear that it is not evidence for individual turns.
fn model_usage(
    session: &ct_domain::AgentSession,
    session_model: Option<&str>,
) -> (Vec<ModelUsageSummary>, usize) {
    let mut counts = BTreeMap::<String, usize>::new();
    let mut unattributed = 0;
    for turn in session.turns() {
        if let Some(model) = &turn.model {
            *counts.entry(model.clone()).or_default() += 1;
        } else {
            unattributed += 1;
        }
    }

    if counts.is_empty() {
        if let Some(model) = session_model {
            counts.entry(model.to_string()).or_default();
        }
    }

    let mut usage: Vec<_> = counts
        .into_iter()
        .map(|(model, turns)| ModelUsageSummary { model, turns })
        .collect();
    usage.sort_by(|left, right| {
        right
            .turns
            .cmp(&left.turns)
            .then_with(|| left.model.cmp(&right.model))
    });
    (usage, unattributed)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionSourceSummary {
    kind: &'static str,
    archived_at: Option<String>,
    redaction: Option<String>,
    differs_from_source: Option<bool>,
}

impl From<&SessionSource> for SessionSourceSummary {
    fn from(value: &SessionSource) -> Self {
        match value {
            SessionSource::Live => Self {
                kind: "live-log",
                archived_at: None,
                redaction: None,
                differs_from_source: None,
            },
            SessionSource::Archive(entry) => Self {
                kind: "archive",
                archived_at: Some(entry.archived_at.to_rfc3339()),
                redaction: Some(entry.redaction.label().to_string()),
                differs_from_source: Some(entry.differs_from_source()),
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributorSummary {
    id: String,
    label: String,
    category: String,
    source: String,
    tokens: u32,
    share: f32,
    confidence: Confidence,
}

/// One context item, for the drill-down behind a category's item count and a
/// contributor's truncated name.
///
/// `contributors` above is capped at the twenty largest, which is the right
/// list to lead with and the wrong one to answer "show me the 19 tool outputs"
/// from: a category can report a count this list cannot enumerate. This is the
/// full inventory of the turn, so the expanded count always matches the row
/// that offered it.
///
/// `category` is the **slug**, matching [`CategorySummary::category`], because
/// that is what the drill-down joins on. [`ContributorSummary::category`] is
/// the human label instead — the two are deliberately different, and mixing
/// them up silently produces empty expansions.
///
/// `share` is a share of the whole turn, never of the category. Re-basing it
/// on the expanded subset is the exact dishonesty `ItemFilter` was built to
/// make unwritable: four tool outputs do not "account for 100% of the
/// context", and the rest of the turn is what the person expanding the row
/// needs to keep in view.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextItemSummary {
    id: String,
    label: String,
    category: String,
    source: String,
    tokens: u32,
    share: f32,
    confidence: Confidence,
    /// Turn at which this item first entered the context, when the log says.
    first_seen_turn: Option<u32>,
    /// Short excerpt the adapters already cap at 160 characters; absent when
    /// the log never carried enough content to quote.
    preview: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySummary {
    category: String,
    label: String,
    tokens: u32,
    share: f32,
    item_count: usize,
    confidence: Confidence,
}

impl From<CategoryBreakdown> for CategorySummary {
    fn from(value: CategoryBreakdown) -> Self {
        Self {
            category: value.category.slug(),
            label: value.category.label().to_string(),
            tokens: value.tokens,
            share: value.share,
            item_count: value.item_count,
            confidence: value.confidence,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextDetail {
    turn: u32,
    model: Option<String>,
    total_tokens: u32,
    residual_tokens: u32,
    residual_is_meaningful: bool,
    context_window: Option<u32>,
    utilisation: Option<f32>,
    calibration_scale: Option<f32>,
    categories: Vec<CategorySummary>,
    contributors: Vec<ContributorSummary>,
    items: Vec<ContextItemSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticItemSummary {
    label: String,
    source: String,
    tokens: u32,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DuplicateSummary {
    copies: usize,
    total_tokens: u32,
    repeated_tokens: u32,
    share: f32,
    confidence: Confidence,
    items: Vec<DiagnosticItemSummary>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LowEntropySummary {
    label: String,
    source: String,
    tokens: u32,
    compression_ratio: f32,
    waste_score_tokens: u32,
    share: f32,
    confidence: Confidence,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretFindingSummary {
    kind: String,
    occurrences: usize,
    turn: Option<u32>,
    line: u32,
    event_type: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    turn: u32,
    duplicate_groups: usize,
    repeated_tokens: u32,
    duplicates: Vec<DuplicateSummary>,
    low_entropy_items: usize,
    waste_score_tokens: u32,
    low_entropy: Vec<LowEntropySummary>,
    secret_findings: usize,
    secret_occurrences: usize,
    scanned_records: usize,
    unreadable_records: usize,
    secrets: Vec<SecretFindingSummary>,
    /// Items this turn held with no content measurement, therefore invisible
    /// to both the duplicate and low-information sections above. See
    /// [`unmeasured_content_items`].
    unmeasured_items: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRunSummary {
    from: u32,
    to: u32,
    turns: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DepartureSummary {
    kind: String,
    turn: Option<u32>,
    reclaimed: Option<u32>,
}

impl From<Departure> for DepartureSummary {
    fn from(value: Departure) -> Self {
        match value {
            Departure::Compaction { turn, reclaimed } => Self {
                kind: "compaction".to_string(),
                turn,
                reclaimed,
            },
            Departure::BranchDiverged { turn } => Self {
                kind: "branch-diverged".to_string(),
                turn: Some(turn),
                reclaimed: None,
            },
            Departure::Unexplained { turn } => Self {
                kind: "unexplained".to_string(),
                turn: Some(turn),
                reclaimed: None,
            },
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleReport {
    id: String,
    label: String,
    category: String,
    source: String,
    first_present: Option<u32>,
    last_present: Option<u32>,
    turns_present: usize,
    runs: Vec<TurnRunSummary>,
    departure: Option<DepartureSummary>,
    still_present: bool,
    unknown_turns: Vec<u32>,
    scanned_turns: usize,
    other_thread_turns: usize,
    last_scanned_turn: Option<u32>,
    recorded_first_seen: Option<u32>,
    first_seen_disagrees: bool,
}

/// Mirrors [`CompactionItemDisposition`]. A discriminated union rather than a
/// flattened `{ kind, historyIndex: Option<u32>, replacementIndex: Option<u32> }`
/// -- the Rust type names its index on the variant that has it precisely so a
/// preserved item's two positions cannot be pulled apart, and this DTO must
/// not reopen that door on the way to JSON. `#[serde(rename_all = "camelCase")]`
/// on the enum only renames the `kind` tag values; each struct variant needs
/// its own `rename_all` to camelCase its own fields (`historyIndex`,
/// `replacementIndex`), which is why it is repeated per variant below rather
/// than assumed to cascade.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CompactionDispositionSummary {
    #[serde(rename_all = "camelCase")]
    Dropped { history_index: u32 },
    #[serde(rename_all = "camelCase")]
    Preserved {
        history_index: u32,
        replacement_index: u32,
    },
    #[serde(rename_all = "camelCase")]
    AddedByReplacement { replacement_index: u32 },
}

impl From<CompactionItemDisposition> for CompactionDispositionSummary {
    fn from(value: CompactionItemDisposition) -> Self {
        match value {
            CompactionItemDisposition::Dropped { history_index } => Self::Dropped { history_index },
            CompactionItemDisposition::Preserved {
                history_index,
                replacement_index,
            } => Self::Preserved {
                history_index,
                replacement_index,
            },
            CompactionItemDisposition::AddedByReplacement { replacement_index } => {
                Self::AddedByReplacement { replacement_index }
            }
        }
    }
}

/// Mirrors [`CompactionDiffUnavailable`], the reason one specific compaction's
/// structural diff could not be produced even though this agent generally
/// supports the feature.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CompactionDiffUnavailableSummary {
    MissingReplacementHistory,
    OversizedRawLine,
    UnavailableRawLine,
    MalformedRawLine,
    MalformedPrecedingItem,
    UnknownPrecedingHistory,
}

impl From<CompactionDiffUnavailable> for CompactionDiffUnavailableSummary {
    fn from(value: CompactionDiffUnavailable) -> Self {
        match value {
            CompactionDiffUnavailable::MissingReplacementHistory => Self::MissingReplacementHistory,
            CompactionDiffUnavailable::OversizedRawLine => Self::OversizedRawLine,
            CompactionDiffUnavailable::UnavailableRawLine => Self::UnavailableRawLine,
            CompactionDiffUnavailable::MalformedRawLine => Self::MalformedRawLine,
            CompactionDiffUnavailable::MalformedPrecedingItem => Self::MalformedPrecedingItem,
            CompactionDiffUnavailable::UnknownPrecedingHistory => Self::UnknownPrecedingHistory,
        }
    }
}

/// Mirrors [`CompactionDiffItem`]. `role` is lowercased to the same
/// vocabulary `ct compactions` prints (`user`/`assistant`/`developer`/
/// `system`) rather than inventing a second spelling for the same fact.
/// `textTokens` stays `null` for opaque or structured items -- see
/// [`CompactionDiffItem::text_tokens`] -- rather than being coerced to `0`,
/// which would misrepresent "not measured" as "measured as empty".
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionDiffItemSummary {
    item_type: String,
    role: Option<String>,
    disposition: CompactionDispositionSummary,
    normalized_json_bytes: u32,
    text_tokens: Option<u32>,
    confidence: Confidence,
}

impl From<CompactionDiffItem> for CompactionDiffItemSummary {
    fn from(value: CompactionDiffItem) -> Self {
        Self {
            item_type: value.item_type,
            role: value.role.map(role_label),
            disposition: value.disposition.into(),
            normalized_json_bytes: value.normalized_json_bytes,
            text_tokens: value.text_tokens.map(|tokens| tokens.tokens()),
            confidence: value.provenance.confidence,
        }
    }
}

fn role_label(role: MessageRole) -> String {
    match role {
        MessageRole::User => "user",
        MessageRole::Assistant => "assistant",
        MessageRole::Developer => "developer",
        MessageRole::System => "system",
    }
    .to_string()
}

/// Mirrors [`CompactionDiff`], plus a third case [`CompactionDiff`] cannot
/// express: an agent that never records a literal replacement history at
/// all. That is [`ct_domain::ports::PortError::Unsupported`] surfacing from
/// the adapter itself, before any single compaction's evidence is even
/// considered -- a different fact from `Unavailable`, which explains why one
/// specific compaction's evidence could not be read on an agent that
/// otherwise supports this. Encoding both as typed success values (never a
/// generic error string) is what lets the desktop panel explain the evidence
/// limit instead of showing an empty view or a spinner that never resolves.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum CompactionDiffSummary {
    #[serde(rename_all = "camelCase")]
    Available {
        turn: Option<u32>,
        line_no: u32,
        items: Vec<CompactionDiffItemSummary>,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        turn: Option<u32>,
        line_no: u32,
        reason: CompactionDiffUnavailableSummary,
    },
    Unsupported {
        detail: String,
    },
}

impl From<CompactionDiff> for CompactionDiffSummary {
    fn from(value: CompactionDiff) -> Self {
        match value {
            CompactionDiff::Available {
                source,
                turn,
                items,
            } => Self::Available {
                turn: turn.map(|turn| turn.get()),
                line_no: source.line_no,
                items: items
                    .into_iter()
                    .map(CompactionDiffItemSummary::from)
                    .collect(),
            },
            CompactionDiff::Unavailable {
                source,
                turn,
                reason,
            } => Self::Unavailable {
                turn: turn.map(|turn| turn.get()),
                line_no: source.line_no,
                reason: reason.into(),
            },
        }
    }
}

/// Mirrors [`ArchiveEntry`]. `agent` is [`AgentKind::label`] -- the same
/// string the session catalog already sends, so a desktop row can match an
/// archive entry to a catalog entry by comparing two plain strings rather
/// than parsing one back into an enum.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveEntrySummary {
    id: String,
    agent: String,
    project: Option<String>,
    archived_at: String,
    redaction: String,
    records: u64,
    source_bytes: u64,
    archived_bytes: u64,
    redacted_records: u64,
    redacted_values: u64,
    differs_from_source: bool,
}

impl From<ArchiveEntry> for ArchiveEntrySummary {
    fn from(value: ArchiveEntry) -> Self {
        Self {
            id: value.id().to_string(),
            agent: value.agent().label().to_string(),
            project: value.descriptor.project.clone(),
            archived_at: value.archived_at.to_rfc3339(),
            redaction: value.redaction.label().to_string(),
            records: value.records,
            source_bytes: value.source_bytes,
            archived_bytes: value.archived_bytes,
            redacted_records: value.redacted_records,
            redacted_values: value.redacted_values,
            differs_from_source: value.differs_from_source(),
        }
    }
}

/// Mirrors [`ArchiveIntegrity`]. A sum type, not a status string with a grab
/// bag of nullable digest fields, for the same reason every other typed
/// refusal in this file is one: the four outcomes call for different
/// reactions, and a reader matching on `kind` cannot reach into
/// `sourceChanged` for a digest that only `archiveDamaged` actually carries.
///
/// `rename_all` is repeated on every struct variant deliberately -- on an
/// enum it renames only the `kind` tag, never a struct variant's own fields.
/// See `ComparabilitySummary` above, which this file has already been bitten
/// by getting this wrong once.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ArchiveIntegritySummary {
    Intact,
    #[serde(rename_all = "camelCase")]
    SourceChanged {
        recorded_digest: String,
        current_digest: String,
        recorded_bytes: u64,
        current_bytes: u64,
    },
    #[serde(rename_all = "camelCase")]
    SourceGone {
        archive_matches_digest: bool,
    },
    #[serde(rename_all = "camelCase")]
    ArchiveDamaged {
        recorded_digest: String,
        current_digest: String,
    },
}

impl From<ArchiveIntegrity> for ArchiveIntegritySummary {
    fn from(value: ArchiveIntegrity) -> Self {
        match value {
            ArchiveIntegrity::Intact => Self::Intact,
            ArchiveIntegrity::SourceChanged {
                recorded_digest,
                current_digest,
                recorded_bytes,
                current_bytes,
            } => Self::SourceChanged {
                recorded_digest,
                current_digest,
                recorded_bytes,
                current_bytes,
            },
            ArchiveIntegrity::SourceGone {
                archive_matches_digest,
            } => Self::SourceGone {
                archive_matches_digest,
            },
            ArchiveIntegrity::ArchiveDamaged {
                recorded_digest,
                current_digest,
            } => Self::ArchiveDamaged {
                recorded_digest,
                current_digest,
            },
        }
    }
}

/// What checking an archived session found, as the desktop reads it.
///
/// `copy_is_sound` and `rebuildable` are read off [`ArchiveIntegrity`] here,
/// not re-derived from `integrity` on the other side of the wire: the domain
/// owns that judgement, and a second implementation of it in TypeScript is
/// exactly how the two would drift.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveVerification {
    integrity: ArchiveIntegritySummary,
    copy_is_sound: bool,
    rebuildable: bool,
}

impl From<ArchiveIntegrity> for ArchiveVerification {
    fn from(value: ArchiveIntegrity) -> Self {
        Self {
            copy_is_sound: value.copy_is_sound(),
            rebuildable: value.rebuildable(),
            integrity: ArchiveIntegritySummary::from(value),
        }
    }
}

/// Everything the archive holds. `root` is the directory copies are written
/// to -- the same string `ct roots` prints -- and `entries` arrive most
/// recently archived first, exactly as [`ContextTrace::archived_sessions`]
/// orders them; this DTO does not re-sort them.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArchiveHolding {
    root: String,
    entries: Vec<ArchiveEntrySummary>,
}

/// What one completed export wrote.
///
/// `redaction` says what was *asked* for and `redactions` how many values were
/// actually replaced. Both, because they answer different questions: the first
/// is whether this file can be shared, the second whether anything in this
/// session was worth redacting. A zero count under `"secrets"` means the
/// scanner found nothing, which is not the same claim as never having looked.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOutcome {
    path: String,
    bytes: u64,
    records: u64,
    redaction: String,
    redactions: usize,
}

/// One recorded instruction attachment, compared with the file on disk now.
///
/// This is a DTO rather than [`ct_application::InstructionFileComparison`] sent
/// straight over the wire for a reason worth stating: that type is also the
/// CLI's and the MCP server's JSON output (`ct-cli/src/mcp.rs`), where its
/// snake_case field names are the published contract. The desktop needs
/// camelCase. Renaming the application type to satisfy this side would quietly
/// break the other, so the boundary converts instead -- the same thing every
/// other DTO in this file does.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionFileComparisonSummary {
    path: String,
    turn: Option<u32>,
    line: u32,
    /// Re-exported unchanged: the application enum already serialises its
    /// variants camelCase, and re-deriving it here would be a second list to
    /// keep in step with the frontend's accepted values.
    status: InstructionFileStatus,
    recorded_digest: Option<String>,
    current_digest: Option<String>,
    recorded_chars: u32,
    current_chars: Option<u32>,
    comparison_basis: String,
    detail: Option<String>,
}

impl From<InstructionFileComparison> for InstructionFileComparisonSummary {
    fn from(value: InstructionFileComparison) -> Self {
        Self {
            path: value.path,
            turn: value.turn,
            line: value.line,
            status: value.status,
            recorded_digest: value.recorded_digest,
            current_digest: value.current_digest,
            recorded_chars: value.recorded_chars,
            current_chars: value.current_chars,
            comparison_basis: value.comparison_basis,
            detail: value.detail,
        }
    }
}

/// The instruction-file comparison for one session.
///
/// `refusal_count` travels rather than being counted from `comparisons` on the
/// far side: a refusal is a comparison this build declined to make, and the
/// number of them is a fact about coverage that a view must be able to state
/// without re-deriving the rule for what counts as one.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionFileReportSummary {
    session_id: String,
    project_root: Option<String>,
    comparisons: Vec<InstructionFileComparisonSummary>,
    refusal_count: usize,
}

impl From<InstructionFileReport> for InstructionFileReportSummary {
    fn from(value: InstructionFileReport) -> Self {
        Self {
            session_id: value.session_id,
            project_root: value.project_root,
            comparisons: value
                .comparisons
                .into_iter()
                .map(InstructionFileComparisonSummary::from)
                .collect(),
            refusal_count: value.refusal_count,
        }
    }
}

/// One category's modelled spend. `cost` is in millionths of a dollar, exactly
/// as [`MoneyMicros`] carries it -- a newtype over `u64`, so it crosses the
/// wire as a bare integer and the frontend divides. Money is not sent as a
/// float and not pre-formatted into a string.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostCategorySummary {
    name: String,
    tokens: u64,
    cost: u64,
    confidence: String,
}

impl From<CostCategory> for CostCategorySummary {
    fn from(value: CostCategory) -> Self {
        Self {
            name: value.name.to_string(),
            tokens: value.tokens,
            cost: value.cost.0,
            confidence: value.confidence.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostTurnSummary {
    turn: u32,
    model: Option<String>,
    priced: bool,
    categories: Vec<CostCategorySummary>,
    total: u64,
}

impl From<CostTurn> for CostTurnSummary {
    fn from(value: CostTurn) -> Self {
        Self {
            turn: value.turn,
            model: value.model,
            priced: value.priced,
            categories: value
                .categories
                .into_iter()
                .map(CostCategorySummary::from)
                .collect(),
            total: value.total.0,
        }
    }
}

/// A turn no rate could be found for, and why.
///
/// Carried separately from `turns` rather than folded in with a zero cost:
/// "this turn cost nothing" and "this turn's model is unpriced" are different
/// claims, and summing the second into a total as though it were the first is
/// how a spend figure comes to understate itself without saying so.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnpricedTurnSummary {
    turn: u32,
    model: Option<String>,
    reason: String,
}

impl From<UnpricedTurn> for UnpricedTurnSummary {
    fn from(value: UnpricedTurn) -> Self {
        Self {
            turn: value.turn,
            model: value.model,
            reason: value.reason.to_string(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostForecastSummary {
    additional_turns: u32,
    average_tokens_per_turn: Vec<CostCategorySummary>,
    projected_additional: u64,
    projected_total: u64,
    assumptions: Vec<String>,
}

impl From<CostForecast> for CostForecastSummary {
    fn from(value: CostForecast) -> Self {
        Self {
            additional_turns: value.additional_turns,
            average_tokens_per_turn: value
                .average_tokens_per_turn
                .into_iter()
                .map(CostCategorySummary::from)
                .collect(),
            projected_additional: value.projected_additional.0,
            projected_total: value.projected_total.0,
            assumptions: value.assumptions,
        }
    }
}

/// The cost report, as the desktop reads it.
///
/// `warning` and `pricing_source` are not decoration: every figure here is
/// modelled from published rates against observed token counts, and the view
/// has to be able to say which rates and what they do not cover.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostReportSummary {
    session_id: String,
    pricing_version: String,
    pricing_source: String,
    warning: String,
    categories: Vec<CostCategorySummary>,
    total: u64,
    turns: Vec<CostTurnSummary>,
    unpriced: Vec<UnpricedTurnSummary>,
    forecast: Option<CostForecastSummary>,
}

impl From<CostReport> for CostReportSummary {
    fn from(value: CostReport) -> Self {
        Self {
            session_id: value.session_id,
            pricing_version: value.pricing_version,
            pricing_source: value.pricing_source,
            warning: value.warning,
            categories: value
                .categories
                .into_iter()
                .map(CostCategorySummary::from)
                .collect(),
            total: value.total.0,
            turns: value.turns.into_iter().map(CostTurnSummary::from).collect(),
            unpriced: value
                .unpriced
                .into_iter()
                .map(UnpricedTurnSummary::from)
                .collect(),
            forecast: value.forecast.map(CostForecastSummary::from),
        }
    }
}

/// One item's membership and size across the two compared turns.
///
/// `token_delta` is `None` when only one side held the item, and
/// `meaningful_token_delta` says whether a delta that does exist survives the
/// instrument bound. Both travel so the view never re-derives the judgement:
/// a number the estimator could have produced on its own must not be read as
/// a content change.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GhostItemSummary {
    id: String,
    label: String,
    category: String,
    source: String,
    left_tokens: Option<u32>,
    right_tokens: Option<u32>,
    token_delta: Option<i64>,
    meaningful_token_delta: bool,
    confidence: Confidence,
}

impl From<GhostItem> for GhostItemSummary {
    fn from(value: GhostItem) -> Self {
        Self {
            id: value.id,
            label: value.label,
            category: value.category.label().to_string(),
            source: value.source,
            left_tokens: value.left_tokens,
            right_tokens: value.right_tokens,
            token_delta: value.token_delta,
            meaningful_token_delta: value.meaningful_token_delta,
            confidence: value.confidence,
        }
    }
}

/// Item identity across two turns, or a refusal to claim it.
///
/// `rename_all` is repeated per variant deliberately, for the reason spelled
/// out on [`ComparabilitySummary`]: on an enum it renames only the variant
/// tag, never each struct-variant's own fields. The application type this
/// converts from has `rename_all` on the enum alone, which is why its
/// `left_turn` reached the frontend unrenamed and every field check failed.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum TemporalGhostSummary {
    #[serde(rename_all = "camelCase")]
    Available {
        left_turn: u32,
        right_turn: u32,
        comparability: ComparabilitySummary,
        gained: Vec<GhostItemSummary>,
        retained: Vec<GhostItemSummary>,
        removed: Vec<GhostItemSummary>,
        assumptions: Vec<String>,
    },
    #[serde(rename_all = "camelCase")]
    Unavailable {
        left_turn: u32,
        right_turn: u32,
        reason: String,
    },
}

impl From<TemporalGhost> for TemporalGhostSummary {
    fn from(value: TemporalGhost) -> Self {
        match value {
            TemporalGhost::Available(available) => Self::Available {
                left_turn: available.left_turn,
                right_turn: available.right_turn,
                comparability: ComparabilitySummary::from(available.comparability),
                gained: available
                    .gained
                    .into_iter()
                    .map(GhostItemSummary::from)
                    .collect(),
                retained: available
                    .retained
                    .into_iter()
                    .map(GhostItemSummary::from)
                    .collect(),
                removed: available
                    .removed
                    .into_iter()
                    .map(GhostItemSummary::from)
                    .collect(),
                assumptions: available.assumptions,
            },
            TemporalGhost::Unavailable {
                left_turn,
                right_turn,
                reason,
            } => Self::Unavailable {
                left_turn,
                right_turn,
                reason,
            },
        }
    }
}

#[tauri::command]
pub fn get_startup(state: tauri::State<'_, AppState>) -> StartupSummary {
    state.startup()
}

/// Search session metadata on the backend and return explicit paging facts.
///
/// Eight parameters for the same reason `AppState::search_sessions` has
/// seven: this is the IPC entry point, so each one is a name the frontend's
/// `invoke` call sends on the wire, not an internal grouping this file is
/// free to tidy away behind a struct.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub fn search_sessions(
    agent: Option<String>,
    query: Option<String>,
    project: Option<ProjectFilter>,
    include_subagents: Option<bool>,
    offset: Option<usize>,
    limit: Option<usize>,
    refresh: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<SessionPage, String> {
    state.search_sessions(
        agent,
        query,
        project,
        include_subagents,
        offset,
        limit,
        refresh,
    )
}

/// The projects a session listing can be narrowed to, with their counts.
#[tauri::command]
pub fn list_projects(
    agent: Option<String>,
    query: Option<String>,
    include_subagents: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<ProjectSummary>, String> {
    state.list_projects(agent, query, include_subagents)
}

#[tauri::command]
pub fn search_memory(
    agent: Option<String>,
    query: String,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<MemoryHit>, String> {
    state.search_memory(agent, query, limit)
}

#[tauri::command]
pub fn inspect_session(
    id: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<SessionDetail, String> {
    state.inspect_session(parse_agent(&agent)?, &id)
}

#[tauri::command]
pub fn get_context(
    id: String,
    agent: String,
    turn: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<ContextDetail, String> {
    state.context(parse_agent(&agent)?, &id, turn)
}

/// The remembered sweep, as it sits on disk.
///
/// Split into a borrowing writer and an owning reader so remembering a sweep
/// does not have to clone a report describing hundreds of sessions.
#[derive(Serialize)]
struct CorpusCacheRef<'a> {
    fingerprint: &'a [ct_domain::SessionFingerprint],
    report: &'a ct_application::CorpusReport,
}

#[derive(Deserialize)]
struct CorpusCache {
    fingerprint: Vec<ct_domain::SessionFingerprint>,
    report: ct_application::CorpusReport,
}

/// Read back the last sweep, but only if it described exactly this corpus.
///
/// Every failure returns `None` and costs a sweep: no file yet, a cache left
/// by an older build whose report shape has since changed, a partial write.
/// None of those deserve an error, because the sweep is always available as
/// the answer -- this is a cache, and a cache that can fail loudly is worse
/// than no cache.
fn read_corpus_cache_file() -> Option<CorpusCache> {
    let raw = fs::read(ct_runtime::corpus_cache_path()).ok()?;
    serde_json::from_slice(&raw).ok()
}

/// The remembered sweep, but only if it still describes this corpus.
fn read_corpus_cache(
    fingerprint: &[ct_domain::SessionFingerprint],
) -> Option<ct_application::CorpusReport> {
    let cached = read_corpus_cache_file()?;
    (cached.fingerprint == fingerprint).then_some(cached.report)
}

/// Remember this sweep for the next launch. Best effort, by design.
///
/// Written to a temporary file and renamed, so a process killed mid-write
/// leaves the previous cache intact rather than a truncated one that would
/// then be parsed and trusted.
fn write_corpus_cache(
    fingerprint: &[ct_domain::SessionFingerprint],
    report: &ct_application::CorpusReport,
) {
    let path = ct_runtime::corpus_cache_path();
    let Some(dir) = path.parent() else { return };
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    let Ok(body) = serde_json::to_vec(&CorpusCacheRef {
        fingerprint,
        report,
    }) else {
        return;
    };
    let scratch = dir.join(format!("report.json.{}.tmp", std::process::id()));
    if fs::write(&scratch, body).is_ok() && fs::rename(&scratch, &path).is_err() {
        let _ = fs::remove_file(&scratch);
    }
}

/// Summarise every local session at once.
///
/// Runs on the command's own thread, which Tauri dispatches off the UI thread,
/// and emits progress as it goes -- a sweep measured at 3.2 seconds over 134
/// local sessions is long enough that a silent wait reads as a hang.
#[tauri::command]
pub fn get_corpus(
    refresh: Option<bool>,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<CorpusSummary, String> {
    state.corpus(refresh.unwrap_or(false), &app)
}

/// The remembered sweep, for painting the dashboard before a real one runs.
#[tauri::command]
pub fn get_corpus_cached(state: tauri::State<'_, AppState>) -> Option<CorpusSummary> {
    state.corpus_remembered()
}

#[tauri::command]
pub fn get_transcript(
    id: String,
    agent: String,
    offset: Option<usize>,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<TranscriptPageSummary, String> {
    state.transcript(parse_agent(&agent)?, &id, offset, limit)
}

#[tauri::command]
pub fn get_transcript_entry(
    id: String,
    agent: String,
    index: usize,
    state: tauri::State<'_, AppState>,
) -> Result<TranscriptEntrySummary, String> {
    state.transcript_entry(parse_agent(&agent)?, &id, index)
}

#[tauri::command]
pub fn run_doctor(
    id: String,
    agent: String,
    turn: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<DoctorReport, String> {
    state.doctor(parse_agent(&agent)?, &id, turn)
}

#[tauri::command]
pub fn get_lifecycle(
    id: String,
    agent: String,
    item: String,
    state: tauri::State<'_, AppState>,
) -> Result<LifecycleReport, String> {
    state.lifecycle(parse_agent(&agent)?, &id, &item)
}

#[tauri::command]
pub fn get_compaction_diff(
    id: String,
    agent: String,
    line_no: u32,
    state: tauri::State<'_, AppState>,
) -> Result<CompactionDiffSummary, String> {
    state.compaction_diff(parse_agent(&agent)?, &id, line_no)
}

#[tauri::command]
pub fn get_turn_diff(
    left_id: String,
    left_agent: String,
    left_turn: u32,
    right_id: String,
    right_agent: String,
    right_turn: u32,
    state: tauri::State<'_, AppState>,
) -> Result<TurnDiffSummary, String> {
    state.turn_diff(
        parse_agent(&left_agent)?,
        &left_id,
        left_turn,
        parse_agent(&right_agent)?,
        &right_id,
        right_turn,
    )
}

#[tauri::command]
pub fn get_instruction_files(
    id: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<InstructionFileReportSummary, String> {
    state.instruction_files(parse_agent(&agent)?, &id)
}

#[tauri::command]
pub fn get_cost(
    id: String,
    agent: String,
    pricing: Option<String>,
    forecast_turns: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<CostReportSummary, String> {
    state.cost(
        parse_agent(&agent)?,
        &id,
        pricing.as_deref(),
        forecast_turns,
    )
}

#[tauri::command]
pub fn get_temporal_ghost(
    id: String,
    agent: String,
    left_turn: u32,
    right_turn: u32,
    state: tauri::State<'_, AppState>,
) -> Result<TemporalGhostSummary, String> {
    state.temporal_ghost(parse_agent(&agent)?, &id, left_turn, right_turn)
}

#[tauri::command]
pub fn get_residual(
    id: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<ResidualReport, String> {
    state.residual(parse_agent(&agent)?, &id)
}

#[tauri::command]
pub fn archived_sessions(state: tauri::State<'_, AppState>) -> Result<ArchiveHolding, String> {
    state.archived_sessions()
}

#[tauri::command]
pub fn archive_session(
    id: String,
    agent: String,
    raw: bool,
    state: tauri::State<'_, AppState>,
) -> Result<ArchiveEntrySummary, String> {
    state.archive_session(parse_agent(&agent)?, &id, raw)
}

#[tauri::command]
pub fn verify_archived(
    id: String,
    agent: String,
    state: tauri::State<'_, AppState>,
) -> Result<ArchiveVerification, String> {
    state.verify_archived(parse_agent(&agent)?, &id)
}

#[tauri::command]
pub fn export_session(
    id: String,
    agent: String,
    redact_secrets: bool,
    state: tauri::State<'_, AppState>,
) -> Result<ExportOutcome, String> {
    state.export_session(parse_agent(&agent)?, &id, redact_secrets)
}

fn format_source(source: &ContextSource) -> String {
    source.to_string()
}

fn parse_agent(agent: &str) -> Result<AgentKind, String> {
    AgentKind::parse(agent)
        .ok_or_else(|| format!("unknown agent '{agent}'; use claude-code or codex"))
}

/// The last path segment, which is what a project is called in the UI.
fn leaf_name(path: &str) -> String {
    path.trim_end_matches(['\\', '/'])
        .rsplit(['\\', '/'])
        .next()
        .filter(|segment| !segment.is_empty())
        .unwrap_or(path)
        .to_string()
}

fn session_matches_query(descriptor: &SessionDescriptor, query: Option<&str>) -> bool {
    let Some(query) = query else {
        return true;
    };
    let needle = query.to_lowercase();
    let agent = match descriptor.agent {
        AgentKind::Codex => "codex",
        AgentKind::ClaudeCode => "claude-code",
    };
    // The title is searched alongside the id, project and path. It is what the
    // row is now labelled with, so a catalog that showed a name it could not
    // then find would be a worse search than the id-only one it replaced.
    let matches = [
        descriptor.id.as_str(),
        descriptor.project.as_deref().unwrap_or_default(),
        descriptor.path.as_str(),
        descriptor
            .title
            .as_ref()
            .map(|title| title.text.as_str())
            .unwrap_or_default(),
        descriptor.git_branch.as_deref().unwrap_or_default(),
        agent,
    ]
    .into_iter()
    .any(|value| value.to_lowercase().contains(&needle));
    matches
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_adapters::{ClaudeCodeAdapter, CodexAdapter, HeuristicEstimator};
    use ct_application::AgentBinding;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT_FIXTURE_HOME: AtomicUsize = AtomicUsize::new(0);

    struct FixtureHomes {
        root: PathBuf,
        codex_home: PathBuf,
        claude_home: PathBuf,
        codex_session: PathBuf,
    }

    impl FixtureHomes {
        fn new() -> Self {
            let unique = format!(
                "context-trace-ui-contract-{}-{}",
                std::process::id(),
                NEXT_FIXTURE_HOME.fetch_add(1, Ordering::Relaxed)
            );
            let root = std::env::temp_dir().join(unique);
            let codex_home = root.join("codex");
            let claude_home = root.join("claude");
            let codex_session = codex_home
                .join("sessions")
                .join("2026")
                .join("07")
                .join("21")
                .join("rollout.jsonl");
            let claude_session = claude_home
                .join("projects")
                .join("C--repos-demo")
                .join("fixture-claude.jsonl");
            let fixtures =
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../tests/fixtures");

            fs::create_dir_all(codex_session.parent().expect("fixture has a parent"))
                .expect("create Codex fixture directory");
            fs::create_dir_all(claude_session.parent().expect("fixture has a parent"))
                .expect("create Claude fixture directory");
            fs::copy(fixtures.join("codex/rollout.jsonl"), &codex_session)
                .expect("stage committed Codex fixture");
            fs::copy(fixtures.join("claude_code/session.jsonl"), claude_session)
                .expect("stage committed Claude Code fixture");

            Self {
                root,
                codex_home,
                claude_home,
                codex_session,
            }
        }

        fn state(&self) -> AppState {
            let app = ContextTrace::new(vec![
                AgentBinding::new(
                    Box::new(ClaudeCodeAdapter::with_home(&self.claude_home)),
                    Box::new(HeuristicEstimator::for_code()),
                ),
                AgentBinding::new(
                    Box::new(CodexAdapter::with_home(&self.codex_home)),
                    Box::new(HeuristicEstimator::for_code()),
                ),
            ]);
            AppState::from_parts(app, vec!["synthetic fixture runtime".to_string()])
        }
    }

    impl Drop for FixtureHomes {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn session_id(sessions: &[SessionSummary], agent: &str) -> String {
        sessions
            .iter()
            .find(|session| session.agent == agent)
            .unwrap_or_else(|| panic!("{agent} synthetic fixture was not listed"))
            .id
            .clone()
    }

    fn all_sessions(state: &AppState) -> Vec<SessionSummary> {
        state
            .search_sessions(
                None,
                None,
                None,
                None,
                Some(0),
                Some(MAX_SESSION_PAGE_SIZE),
                None,
            )
            .expect("list committed synthetic fixtures")
            .sessions
    }

    /// An estimator that leaves `chars_per_token` at the port's default
    /// `None` -- what a real tokenizer reports, unlike every
    /// `HeuristicEstimator` this file otherwise wires. Exists only so an
    /// `incomparable` cross-session diff is reachable in a test:
    /// `FixtureHomes::state` wires the same heuristic to both agents on
    /// purpose, so it alone can never produce this arm.
    struct FakeTokenizer;

    impl TokenEstimator for FakeTokenizer {
        fn count_text(&self, text: &str) -> ct_domain::TokenCount {
            ct_domain::TokenCount::estimated(text.chars().count() as u32)
        }

        fn estimate_from_chars(&self, char_len: u32) -> ct_domain::TokenCount {
            ct_domain::TokenCount::estimated(char_len)
        }

        fn name(&self) -> &str {
            "fake-tokenizer"
        }
    }

    /// Discovery-only catalog used to exercise paging without creating hundreds
    /// of on-disk JSONL files. The page endpoint must not parse a body merely
    /// to make an older session searchable.
    struct CatalogAdapter {
        sessions: Vec<SessionDescriptor>,
    }

    impl ct_domain::ports::AgentAdapter for CatalogAdapter {
        fn agent(&self) -> AgentKind {
            AgentKind::Codex
        }

        fn roots(&self) -> Vec<String> {
            Vec::new()
        }

        fn discover(&self) -> ct_domain::ports::PortResult<Vec<SessionDescriptor>> {
            Ok(self.sessions.clone())
        }

        fn load(
            &self,
            _descriptor: &SessionDescriptor,
        ) -> ct_domain::ports::PortResult<ct_domain::AgentSession> {
            Err(ct_domain::ports::PortError::Unsupported(
                "catalog test only supports discovery".into(),
            ))
        }

        fn reconstruct(
            &self,
            _session: &ct_domain::AgentSession,
            _turn: TurnNumber,
            _estimator: &dyn ct_domain::ports::TokenEstimator,
        ) -> ct_domain::ports::PortResult<ct_domain::ports::ReconstructedContext> {
            Err(ct_domain::ports::PortError::Unsupported(
                "catalog test only supports discovery".into(),
            ))
        }
    }

    /// Wraps a real adapter but reports its first discovered session under a
    /// fixed id, so two different agents can be made to genuinely collide on
    /// the same id string for `resolve`.
    struct CollidingIdAdapter {
        inner: Box<dyn ct_domain::ports::AgentAdapter>,
        id: ct_domain::SessionId,
    }

    impl ct_domain::ports::AgentAdapter for CollidingIdAdapter {
        fn agent(&self) -> AgentKind {
            self.inner.agent()
        }

        fn roots(&self) -> Vec<String> {
            self.inner.roots()
        }

        fn discover(&self) -> ct_domain::ports::PortResult<Vec<SessionDescriptor>> {
            Ok(self
                .inner
                .discover()?
                .into_iter()
                .take(1)
                .map(|mut descriptor| {
                    descriptor.id = self.id.clone();
                    descriptor
                })
                .collect())
        }

        fn load(
            &self,
            descriptor: &SessionDescriptor,
        ) -> ct_domain::ports::PortResult<ct_domain::AgentSession> {
            self.inner.load(descriptor)
        }

        fn reconstruct(
            &self,
            session: &ct_domain::AgentSession,
            turn: TurnNumber,
            estimator: &dyn ct_domain::ports::TokenEstimator,
        ) -> ct_domain::ports::PortResult<ct_domain::ports::ReconstructedContext> {
            self.inner.reconstruct(session, turn, estimator)
        }
    }

    /// Both fixture sessions reported under the same id, one per agent, with
    /// the fixture homes kept alive for the caller to hold.
    fn colliding_id_state() -> (AppState, FixtureHomes, String) {
        let homes = FixtureHomes::new();
        let id = ct_domain::SessionId::new("collision-id").unwrap();
        let app = ContextTrace::new(vec![
            AgentBinding::new(
                Box::new(CollidingIdAdapter {
                    inner: Box::new(ClaudeCodeAdapter::with_home(&homes.claude_home)),
                    id: id.clone(),
                }),
                Box::new(HeuristicEstimator::for_code()),
            ),
            AgentBinding::new(
                Box::new(CollidingIdAdapter {
                    inner: Box::new(CodexAdapter::with_home(&homes.codex_home)),
                    id: id.clone(),
                }),
                Box::new(HeuristicEstimator::for_code()),
            ),
        ]);
        let state = AppState::from_parts(app, Vec::new());
        (state, homes, id.to_string())
    }

    fn catalog_descriptor(index: usize, project: &str) -> SessionDescriptor {
        SessionDescriptor {
            id: ct_domain::SessionId::new(format!("catalog-{index:04}")).unwrap(),
            agent: AgentKind::Codex,
            path: format!("C:/catalog/session-{index:04}.jsonl"),
            size_bytes: 1,
            project: Some(project.to_string()),
            title: None,
            git_branch: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        }
    }

    /// A catalog with real variation along every axis the project and
    /// subagent filters branch on: two distinct projects, a root thread and
    /// its subagent in one of them, and a session whose log recorded no
    /// project at all. `catalog_descriptor` alone cannot build this -- every
    /// descriptor it returns is `ThreadRole::Root` with `project: Some(_)` --
    /// so the fixture tests that need a subagent or an unrecorded project
    /// construct descriptors directly, the same way
    /// `session_summary_reports_a_subagent_thread_and_its_parent` does.
    fn filter_fixture_descriptors() -> Vec<SessionDescriptor> {
        fn descriptor(
            id: &str,
            project: Option<&str>,
            thread_role: ThreadRole,
        ) -> SessionDescriptor {
            SessionDescriptor {
                id: ct_domain::SessionId::new(id).unwrap(),
                agent: AgentKind::Codex,
                path: format!("C:/catalog/{id}.jsonl"),
                size_bytes: 1,
                project: project.map(str::to_string),
                title: None,
                git_branch: None,
                started_at: None,
                last_activity: None,
                thread_role,
            }
        }
        vec![
            descriptor("root-alpha", Some("C:/repos/alpha"), ThreadRole::Root),
            descriptor(
                "subagent-alpha",
                Some("C:/repos/alpha"),
                ThreadRole::Subagent {
                    parent: ct_domain::SessionId::new("root-alpha").unwrap(),
                },
            ),
            descriptor("root-beta", Some("C:/repos/beta"), ThreadRole::Root),
            descriptor("root-unrecorded", None, ThreadRole::Root),
        ]
    }

    fn catalog_state(sessions: Vec<SessionDescriptor>) -> AppState {
        AppState::from_parts(
            ContextTrace::new(vec![AgentBinding::new(
                Box::new(CatalogAdapter { sessions }),
                Box::new(HeuristicEstimator::for_code()),
            )]),
            Vec::new(),
        )
    }

    fn assert_context_contract(state: &AppState, id: &str, agent: &str, model: &str) {
        let kind = AgentKind::parse(agent).expect("valid agent label in test");
        let detail = state
            .inspect_session(kind, id)
            .expect("inspect fixture session");
        assert_eq!(detail.session.agent, agent);
        assert_eq!(detail.model.as_deref(), Some(model));
        assert!(detail.model_usage.iter().any(|entry| entry.model == model));
        assert_eq!(detail.unattributed_model_turns, 0);
        let detail_json = serde_json::to_value(&detail).expect("detail serializes for IPC");
        assert!(detail_json["modelUsage"].is_array());
        assert!(detail_json["unattributedModelTurns"].is_number());
        assert!(detail_json.get("model_usage").is_none());
        assert!(detail.turn_count > 0);
        assert!(detail.event_count > 0);
        let peak_turn = detail.peak_turn.expect("fixture has prompt usage");

        let context = state
            .context(kind, id, None)
            .expect("load context for the peak turn");
        assert_eq!(context.turn, peak_turn);
        assert_eq!(context.model.as_deref(), Some(model));
        assert!(context.total_tokens > 0);
        assert!(!context.categories.is_empty());
        assert!(!context.contributors.is_empty());
        let item_id = context.contributors[0].id.clone();

        assert!(!context.items.is_empty());
        // The drill-down behind a category's item count is only trustworthy if
        // it can enumerate every item that count refers to. `contributors` is
        // capped at twenty and cannot; `items` is the whole turn.
        //
        // `unattributed` is the one row this does not hold for, and
        // deliberately so: the domain reports the unlogged remainder as a
        // single synthetic item so it keeps a share of the total, but there is
        // no logged item behind it to enumerate. A view that expanded it into
        // an empty list would be claiming the remainder is nothing, which is
        // the opposite of what it measures.
        for category in &context.categories {
            if category.category == "unattributed" {
                continue;
            }
            let listed = context
                .items
                .iter()
                .filter(|item| item.category == category.category)
                .count();
            assert_eq!(
                listed, category.item_count,
                "category {} promises {} items and lists {listed}",
                category.category, category.item_count
            );
        }

        let json = serde_json::to_value(&context).expect("context detail serializes for IPC");
        assert!(json["totalTokens"].is_number());
        assert!(json["residualIsMeaningful"].is_boolean());
        assert!(json.get("total_tokens").is_none());
        let items = json["items"].as_array().expect("items reach the desktop");
        assert!(!items.is_empty());
        assert!(items[0].get("firstSeenTurn").is_some());
        assert!(items[0].get("first_seen_turn").is_none());
        // The join key. `CategorySummary.category` is a slug and
        // `ContributorSummary.category` is a human label; an item carrying the
        // label instead would silently expand to nothing on the frontend, so
        // pin it to the value the drill-down actually looks up.
        let slugs: Vec<&str> = json["categories"]
            .as_array()
            .expect("categories reach the desktop")
            .iter()
            .map(|category| category["category"].as_str().expect("slug is a string"))
            .collect();
        for item in items {
            let category = item["category"]
                .as_str()
                .expect("item category is a string");
            assert!(
                slugs.contains(&category),
                "item category {category:?} matches no category row {slugs:?}"
            );
        }

        let doctor = state
            .doctor(kind, id, Some(peak_turn))
            .expect("run content diagnostics for the peak turn");
        assert_eq!(doctor.turn, peak_turn);
        assert!(doctor.scanned_records > 0);
        let json = serde_json::to_value(doctor).expect("doctor report serializes for IPC");
        assert!(json["duplicates"].is_array());
        assert!(json["lowEntropy"].is_array());
        assert!(json["secrets"].is_array());
        assert!(json.get("secret_occurrences").is_none());
        assert!(json["unmeasuredItems"].is_number());
        assert!(json.get("unmeasured_items").is_none());

        let lifecycle = state
            .lifecycle(kind, id, &item_id)
            .expect("trace a listed context contributor");
        assert_eq!(lifecycle.id, item_id);
        assert!(lifecycle.turns_present > 0);
        let json = serde_json::to_value(lifecycle).expect("lifecycle serializes for IPC");
        assert!(json["runs"].is_array());
        assert!(json["stillPresent"].is_boolean());
        assert!(json.get("first_present").is_none());
    }

    /// The three payloads that shipped `ct-application` types straight over
    /// IPC, and so reached the desktop in snake_case while every runtime
    /// validator on the other side required camelCase. Each one failed with
    /// "ContextTrace received an invalid response from ...".
    ///
    /// This asserts the wire format rather than the Rust struct because the
    /// wire format is what broke: the structs were always correct, and a
    /// `serde` attribute is the only thing standing between them and a
    /// regression. `rename_all` on a tagged enum renames the variant tag and
    /// not the fields inside it, which is exactly how the temporal ghost's
    /// `left_turn` escaped -- so that one is checked in both variants.
    #[test]
    fn evidence_payloads_reach_the_desktop_in_camel_case() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let id = session_id(&sessions, "codex");
        let kind = AgentKind::Codex;

        let files = state
            .instruction_files(kind, &id)
            .expect("compare instruction files for the fixture session");
        let json = serde_json::to_value(&files).expect("instruction files serialize for IPC");
        assert!(json["sessionId"].is_string());
        assert!(json["comparisons"].is_array());
        assert!(json["refusalCount"].is_number());
        assert!(json.get("session_id").is_none());
        assert!(json.get("project_root").is_none());
        assert!(json.get("refusal_count").is_none());
        for comparison in json["comparisons"].as_array().expect("comparisons array") {
            assert!(comparison.get("recordedChars").is_some());
            assert!(comparison.get("comparisonBasis").is_some());
            assert!(comparison.get("recorded_chars").is_none());
            assert!(comparison.get("comparison_basis").is_none());
        }

        let cost = state
            .cost(kind, &id, None, Some(5))
            .expect("project cost for the fixture session");
        let json = serde_json::to_value(&cost).expect("cost report serializes for IPC");
        assert!(json["sessionId"].is_string());
        assert!(json["pricingVersion"].is_string());
        assert!(json["pricingSource"].is_string());
        // Money crosses as a bare integer count of millionths, not a float and
        // not a pre-formatted string.
        assert!(json["total"].is_u64());
        assert!(json.get("pricing_version").is_none());
        assert!(json.get("pricing_source").is_none());
        if let Some(forecast) = json.get("forecast").filter(|value| !value.is_null()) {
            assert!(forecast["additionalTurns"].is_number());
            assert!(forecast["projectedTotal"].is_u64());
            assert!(forecast["averageTokensPerTurn"].is_array());
            assert!(forecast.get("additional_turns").is_none());
            assert!(forecast.get("projected_total").is_none());
        }

        let detail = state.inspect_session(kind, &id).expect("inspect fixture");
        let peak = detail.peak_turn.expect("fixture has prompt usage");
        let ghost = state
            .temporal_ghost(kind, &id, 1, peak)
            .expect("reconstruct the temporal ghost across two turns");
        let json = serde_json::to_value(&ghost).expect("temporal ghost serializes for IPC");
        // Both variants carry the turn pair, and both must camelCase it: the
        // frontend validates the unavailable branch just as strictly.
        assert!(json["leftTurn"].is_number());
        assert!(json["rightTurn"].is_number());
        assert!(json.get("left_turn").is_none());
        assert!(json.get("right_turn").is_none());
        assert!(matches!(
            json["status"].as_str(),
            Some("available") | Some("unavailable")
        ));
        if json["status"] == "available" {
            for group in ["gained", "retained", "removed"] {
                for item in json[group].as_array().expect("ghost group is an array") {
                    assert!(item.get("meaningfulTokenDelta").is_some());
                    assert!(item.get("leftTokens").is_some());
                    assert!(item.get("meaningful_token_delta").is_none());
                    assert!(item.get("left_tokens").is_none());
                }
            }
        }
    }

    #[test]
    fn model_usage_is_sorted_and_keeps_unattributed_turns_visible() {
        let turn = |number: u32, model: Option<&str>| ct_domain::Turn {
            number: TurnNumber::new(number).unwrap(),
            timestamp: None,
            model: model.map(str::to_string),
            usage: Default::default(),
            event_indices: Vec::new(),
            anchor_index: None,
        };
        let session = ct_domain::AgentSession::new(
            ct_domain::SessionId::new("model-usage").unwrap(),
            AgentKind::Codex,
            Default::default(),
            Vec::new(),
            vec![
                turn(1, Some("zeta")),
                turn(2, Some("alpha")),
                turn(3, None),
                turn(4, Some("alpha")),
                turn(5, Some("beta")),
                turn(6, Some("zeta")),
            ],
            Vec::new(),
        );

        let (usage, unattributed) = model_usage(&session, None);
        assert_eq!(
            usage
                .iter()
                .map(|entry| (entry.model.as_str(), entry.turns))
                .collect::<Vec<_>>(),
            vec![("alpha", 2), ("zeta", 2), ("beta", 1)]
        );
        assert_eq!(unattributed, 1);
    }

    #[test]
    fn session_summary_uses_stable_ui_strings() {
        let descriptor = SessionDescriptor {
            id: ct_domain::SessionId::new("abc123").unwrap(),
            agent: AgentKind::Codex,
            path: "session.jsonl".to_string(),
            size_bytes: 42,
            project: Some("ContextTrace".to_string()),
            title: None,
            git_branch: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        };

        let summary = SessionSummary::from(descriptor);
        assert_eq!(summary.id, "abc123");
        assert_eq!(summary.agent, "codex");
        assert_eq!(summary.size_bytes, 42);
        assert_eq!(summary.thread_role.kind, "root");
        assert_eq!(summary.thread_role.parent, None);
    }

    #[test]
    fn session_summary_reports_a_subagent_thread_and_its_parent() {
        let descriptor = SessionDescriptor {
            id: ct_domain::SessionId::new("child-id").unwrap(),
            agent: AgentKind::Codex,
            path: "session.jsonl".to_string(),
            size_bytes: 42,
            project: Some("ContextTrace".to_string()),
            title: None,
            git_branch: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Subagent {
                parent: ct_domain::SessionId::new("root-id").unwrap(),
            },
        };

        let summary = SessionSummary::from(descriptor);
        assert_eq!(summary.thread_role.kind, "subagent");
        assert_eq!(summary.thread_role.parent.as_deref(), Some("root-id"));

        let json = serde_json::to_value(&summary).expect("session summary serializes for IPC");
        assert_eq!(json["threadRole"]["kind"], "subagent");
        assert_eq!(json["threadRole"]["parent"], "root-id");
    }

    #[test]
    fn fixture_backed_list_inspect_and_context_contract_covers_both_agents() {
        let homes = FixtureHomes::new();
        let state = homes.state();

        let startup = state.startup();
        assert_eq!(startup.roots.len(), 2);
        assert_eq!(startup.warnings, ["synthetic fixture runtime"]);

        let sessions = all_sessions(&state);
        assert_eq!(sessions.len(), 2);
        let codex_id = session_id(&sessions, "codex");
        let claude_id = session_id(&sessions, "claude-code");

        assert_context_contract(&state, &codex_id, "codex", "gpt-5-codex");
        assert_context_contract(&state, &claude_id, "claude-code", "claude-opus-4-8");
    }

    #[test]
    fn subagent_threads_are_left_out_unless_they_are_asked_for() {
        // Four descriptors: two root sessions in different projects, one
        // root session with no recorded project, and one subagent under
        // `root-alpha`. Only the subagent's presence or absence can move
        // these totals, so a no-op filter is caught by the exact counts
        // below, not merely by an inequality both sides of a real filter
        // would also satisfy.
        let state = catalog_state(filter_fixture_descriptors());
        let without = state
            .search_sessions(None, None, None, None, None, None, None)
            .expect("a page");
        let with = state
            .search_sessions(None, None, None, Some(true), None, None, None)
            .expect("a page");
        assert_eq!(
            without.total, 3,
            "three root sessions; the subagent thread is left out by default"
        );
        assert_eq!(
            with.total, 4,
            "asking for subagents adds exactly the one subagent thread"
        );
        assert!(
            without
                .sessions
                .iter()
                .all(|session| session.thread_role.kind == "root"),
            "a subagent thread is not a run the reader started"
        );
        assert!(
            with.sessions
                .iter()
                .any(|session| session.thread_role.kind == "subagent"),
            "asking for subagents must actually surface one"
        );
    }

    #[test]
    fn a_project_filter_matches_the_whole_path_and_narrows_the_total() {
        // Two sessions carry "C:/repos/alpha" (one root, one subagent), one
        // carries "C:/repos/beta", and one has no recorded project. With
        // subagents left out by default, filtering to alpha must leave
        // exactly the one root session in it -- a no-op filter would instead
        // return all three root sessions, and a substring-matching filter
        // would wrongly also catch nothing here since neither project name
        // is a prefix of the other, so this alone would not catch that bug;
        // the exact-match assertion below is what does.
        let state = catalog_state(filter_fixture_descriptors());
        let all = state
            .search_sessions(None, None, None, None, None, None, None)
            .expect("a page");
        assert_eq!(
            all.total, 3,
            "three root sessions before any project filter"
        );

        let filtered = state
            .search_sessions(
                None,
                None,
                Some(ProjectFilter::Path("C:/repos/alpha".to_string())),
                None,
                None,
                None,
                None,
            )
            .expect("a page");
        assert_eq!(
            filtered.total, 1,
            "only root-alpha matches; root-beta and the unrecorded-project \
             session must be excluded, not merely outnumbered"
        );
        assert!(
            filtered
                .sessions
                .iter()
                .all(|session| session.project.as_deref() == Some("C:/repos/alpha")),
            "the filter is exact, not a substring: two checkouts can share a leaf name"
        );
    }

    #[test]
    fn the_project_list_counts_what_selecting_it_would_show() {
        // Exercises all three project shapes at once: two named projects and
        // the `Unrecorded` case (root-unrecorded's log kept no `cwd`), which
        // nothing before this fix ever put through `list_projects` or the
        // `ProjectFilter::Unrecorded` arm of `search_sessions`.
        let state = catalog_state(filter_fixture_descriptors());
        let projects = state.list_projects(None, None, None).expect("projects");
        assert_eq!(projects.len(), 3, "alpha, beta, and the unrecorded bucket");
        assert!(
            projects.iter().any(|summary| summary.path.is_none()),
            "the unrecorded-project session must get its own entry"
        );
        for summary in &projects {
            let filtered = state
                .search_sessions(
                    None,
                    None,
                    Some(match &summary.path {
                        Some(path) => ProjectFilter::Path(path.clone()),
                        None => ProjectFilter::Unrecorded,
                    }),
                    None,
                    None,
                    None,
                    None,
                )
                .expect("a page");
            assert_eq!(filtered.total, summary.count, "{:?}", summary.label);
        }
    }

    #[test]
    fn the_project_list_narrows_with_the_same_query_the_session_list_used() {
        // Regression for a dropdown that read "contexttrace · 41" while the
        // search box had already narrowed the visible list to 3: the count
        // beside an option must describe what selecting it would show given
        // the query already typed, not the whole unfiltered catalog. Only
        // "root-alpha" and "subagent-alpha" have "alpha" in their id, path or
        // project; "root-beta" and "root-unrecorded" have it in none of
        // those, so a query-blind count would still report all three
        // projects instead of just alpha's.
        let state = catalog_state(filter_fixture_descriptors());
        let projects = state
            .list_projects(None, Some("alpha".to_string()), None)
            .expect("projects");
        assert_eq!(
            projects,
            vec![ProjectSummary {
                label: "alpha".to_string(),
                path: Some("C:/repos/alpha".to_string()),
                count: 1,
            }],
            "only the alpha project should survive the query, with its \
             subagent-excluded count of 1, not the unfiltered catalog"
        );
    }

    #[test]
    fn backend_search_reaches_a_targeted_session_after_the_first_500() {
        let sessions = (0..501)
            .map(|index| {
                let project = if index == 500 {
                    "Targeted older project"
                } else {
                    "Ordinary project"
                };
                catalog_descriptor(index, project)
            })
            .collect();
        let state = catalog_state(sessions);

        let first_page = state
            .search_sessions(
                Some("codex".into()),
                None,
                None,
                None,
                Some(0),
                Some(500),
                None,
            )
            .expect("list the first page");
        assert_eq!(first_page.total, 501);
        assert_eq!(first_page.sessions.len(), 500);
        assert!(
            first_page.has_more,
            "the first page must not impersonate the catalog"
        );

        let targeted = state
            .search_sessions(
                None,
                Some("  TARGETED older  ".into()),
                None,
                None,
                Some(0),
                Some(50),
                None,
            )
            .expect("search the complete catalog before paging");
        assert_eq!(targeted.total, 1);
        assert_eq!(targeted.sessions.len(), 1);
        assert!(!targeted.has_more);
        assert_eq!(targeted.sessions[0].id, "catalog-0500");

        let final_page = state
            .search_sessions(None, None, None, None, Some(500), Some(50), None)
            .expect("page beyond the legacy 500-item cutoff");
        assert_eq!(final_page.total, 501);
        assert_eq!(final_page.offset, 500);
        assert_eq!(final_page.sessions.len(), 1);
        assert!(!final_page.has_more);

        let json = serde_json::to_value(targeted).expect("page serializes for IPC");
        assert_eq!(json["total"], 1);
        assert_eq!(json["offset"], 0);
        assert_eq!(json["hasMore"], false);
        assert!(json["sessions"].is_array());
    }

    #[test]
    fn paging_through_results_does_not_evict_already_parsed_sessions() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("first inspection populates the cache");
        fs::remove_file(&homes.codex_session).expect("remove staged fixture after caching it");

        // An ordinary page request (offset > 0, no explicit refresh) is what
        // "Load more" sends. It must not throw away the parse above.
        state
            .search_sessions(None, None, None, None, Some(1), Some(1), None)
            .expect("page through the catalog without asking for a refresh");
        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("pagination does not evict a session already parsed");

        // Nor does an ordinary offset-0 search issued without `refresh`, e.g.
        // a query or filter change.
        state
            .search_sessions(None, Some("".into()), None, None, Some(0), Some(50), None)
            .expect("search again without asking for a refresh");
        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("an unrelated search does not evict a session already parsed");
    }

    #[test]
    fn an_explicit_refresh_evicts_sessions_that_disappeared_from_disk() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("first inspection populates the cache");
        fs::remove_file(&homes.codex_session).expect("remove staged fixture after caching it");
        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("cached inspection does not reread a session until an explicit refresh");

        let refreshed = state
            .search_sessions(None, None, None, None, Some(0), None, Some(true))
            .expect("an explicit refresh clears the cache and rescans the catalog");
        assert_eq!(refreshed.sessions.len(), 1);
        assert_eq!(refreshed.sessions[0].agent, "claude-code");
        let error = match state.inspect_session(AgentKind::Codex, &codex_id) {
            Err(error) => error,
            Ok(_) => panic!("a genuine refresh evicts sessions that have disappeared from disk"),
        };
        assert!(error.contains("no session matching"));
    }

    #[test]
    fn session_cache_capacity_is_bounded_and_evicts_least_recently_used() {
        let mut cache: BoundedCache<u32, u32> = BoundedCache::new(SESSION_CACHE_CAPACITY);
        for key in 0..(SESSION_CACHE_CAPACITY as u32 + 3) {
            cache.insert(key, key * 10);
        }
        assert_eq!(
            cache.len(),
            SESSION_CACHE_CAPACITY,
            "the cache never grows past its capacity"
        );
        assert!(
            cache.get(&0).is_none(),
            "the oldest untouched entry was evicted first"
        );
        let newest = SESSION_CACHE_CAPACITY as u32 + 2;
        assert_eq!(
            cache.get(&newest),
            Some(newest * 10),
            "the most recently inserted entry survives"
        );

        // Touching an entry protects it from the next eviction.
        let mut cache: BoundedCache<u32, u32> = BoundedCache::new(3);
        cache.insert(1, 100);
        cache.insert(2, 200);
        cache.insert(3, 300);
        assert_eq!(cache.get(&1), Some(100)); // touch 1, so 2 becomes least recently used
        cache.insert(4, 400); // capacity exceeded: evicts 2, not the touched 1
        assert!(cache.get(&2).is_none(), "the untouched entry was evicted");
        assert_eq!(cache.get(&1), Some(100), "the touched entry survived");
        assert_eq!(cache.get(&4), Some(400), "the newest entry survived");

        assert_eq!(cache.remove(&1), Some(100));
        assert!(
            cache.get(&1).is_none(),
            "a targeted invalidation removes the value"
        );
        cache.insert(5, 500);
        assert_eq!(cache.len(), 3, "removing a key also removes its LRU entry");
    }

    #[test]
    fn agent_and_id_together_identify_a_session_when_ids_collide_across_agents() {
        let (state, _homes, id) = colliding_id_state();

        let page = state
            .search_sessions(None, None, None, None, Some(0), None, None)
            .expect("list sessions across both agents");
        assert_eq!(
            page.sessions.len(),
            2,
            "both agents report a session under the same id"
        );
        assert!(page.sessions.iter().all(|session| session.id == id));
        let agents: std::collections::BTreeSet<&str> = page
            .sessions
            .iter()
            .map(|session| session.agent.as_str())
            .collect();
        assert_eq!(
            agents.len(),
            2,
            "the collision is across two genuinely distinct agents"
        );

        // The bindings are registered Claude Code first, matching the real
        // composition root (`ct_runtime::build`), so an id-only lookup would
        // answer with this one whichever session was asked for.
        let claude = state
            .inspect_session(AgentKind::ClaudeCode, &id)
            .expect("resolve the Claude Code side of the collision");
        assert_eq!(claude.session.agent, "claude-code");

        // The second session is the one the bug made unreachable. Scoping the
        // lookup to its agent must open that session itself, not the first
        // binding's session wearing a Codex label.
        let codex = state
            .inspect_session(AgentKind::Codex, &id)
            .expect("the later binding's session is reachable when the agent scopes the lookup");
        assert_eq!(codex.session.agent, "codex");

        // Same id, two agents, two genuinely different sessions. The event
        // counts come from the two distinct fixtures, so an answer that
        // silently resolved to the wrong side would show up here.
        assert_ne!(
            claude.event_count, codex.event_count,
            "each agent's own session was loaded, not the same one twice"
        );
    }

    #[test]
    fn analysis_holds_a_session_handle_rather_than_the_cache_lock() {
        // Tauri dispatches commands on separate threads. A handle that outlives
        // the lock is what lets a doctor run over a large session proceed while
        // the session list answers on another thread.
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        let cached = state
            .cached_session(AgentKind::Codex, &codex_id, true)
            .expect("load the session for analysis");
        assert!(
            state.sessions.try_lock().is_ok(),
            "no cache lock may be held while a session handle is in use"
        );

        let raw = ct_runtime::raw_event_source(&cached.descriptor.path);
        let scan = state.app.scan_secrets(&cached.session, &raw);
        assert!(scan.scanned_records > 0);
        assert!(
            state.sessions.try_lock().is_ok(),
            "the raw-file secret scan must not run under the cache lock"
        );
    }

    #[test]
    fn a_panic_under_the_cache_lock_does_not_disable_later_commands() {
        // Before, the closure ran under the lock, so one panic anywhere in
        // analysis left every later command answering "the in-memory session
        // cache is unavailable" until the app was restarted.
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = state.sessions.lock().expect("take the cache lock");
            panic!("a command unwound while holding the cache lock");
        }));
        std::panic::set_hook(previous);
        assert!(panicked.is_err());
        assert!(
            state.sessions.is_poisoned(),
            "the test must actually poison the lock it is about"
        );

        let after = all_sessions(&state);
        assert_eq!(after.len(), 2);
        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("inspection still answers after a poisoning panic");
        state
            .doctor(AgentKind::Codex, &codex_id, None)
            .expect("the doctor still answers after a poisoning panic");
    }

    #[test]
    fn service_reports_actionable_invalid_agent_session_and_turn_errors() {
        let homes = FixtureHomes::new();
        let state = homes.state();

        let error = match state.search_sessions(
            Some("cursor".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        ) {
            Err(error) => error,
            Ok(_) => panic!("unsupported agents are rejected before discovery"),
        };
        assert_eq!(error, "unknown agent 'cursor'; use claude-code or codex");

        let error = match state.inspect_session(AgentKind::Codex, "does-not-exist") {
            Err(error) => error,
            Ok(_) => panic!("missing sessions are reported to the desktop client"),
        };
        assert_eq!(error, "no session matching 'does-not-exist'");

        let sessions = state
            .search_sessions(
                Some("codex".to_string()),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .expect("list Codex fixture")
            .sessions;
        let codex_id = session_id(&sessions, "codex");
        let error = match state.context(AgentKind::Codex, &codex_id, Some(999)) {
            Err(error) => error,
            Ok(_) => panic!("out-of-range turns are not silently substituted"),
        };
        assert_eq!(
            error,
            "turn 999 is out of range; this session has 2 turn(s)"
        );
    }

    /// Two turns of one session -- the commonest comparison, and the one that
    /// must stay cheap now that `turn_diff` also accepts two different
    /// sessions. Both sides come from one load, so one instrument sizes them
    /// and `Comparability` is `identical`; the cross-session arms are pinned
    /// separately below, where they are actually reachable.
    #[test]
    fn a_turn_diff_reports_one_instrument_and_camelcases_every_field() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        let detail = state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("codex fixture inspects");
        let turns: Vec<u32> = detail
            .growth
            .iter()
            .filter(|point| point.prompt_tokens.is_some())
            .map(|point| point.turn)
            .collect();
        assert!(
            turns.len() >= 2,
            "the comparison needs two measured turns; the fixture has {}",
            turns.len()
        );

        let diff = state
            .turn_diff(
                AgentKind::Codex,
                &codex_id,
                turns[0],
                AgentKind::Codex,
                &codex_id,
                turns[turns.len() - 1],
            )
            .expect("two turns of one session compare");
        let json = serde_json::to_value(&diff).expect("serialises");

        // Both sides come from one session, so one instrument sized them. This
        // is the assertion that would fail first if the desktop ever started
        // building its two sides from separately calibrated loads.
        assert_eq!(json["comparability"]["kind"], "identical");
        assert!(json["comparability"]["estimator"].is_string());

        // Each side now names the session it came from, which matters once
        // the two sides can genuinely differ.
        assert_eq!(json["left"]["id"], codex_id);
        assert_eq!(json["left"]["agent"], "codex");
        assert_eq!(json["right"]["id"], codex_id);
        assert_eq!(json["right"]["agent"], "codex");

        assert_eq!(json["left"]["turn"], turns[0]);
        assert_eq!(json["right"]["turn"], turns[turns.len() - 1]);
        assert!(json["promptDelta"].is_i64());
        assert!(json["totalsAreObserved"].is_boolean());
        assert!(json.get("prompt_delta").is_none());
        assert!(json.get("totals_are_observed").is_none());

        // `rename_all` on an enum renames only the variant tag, so a struct
        // variant's own fields need their own attribute. Assert the absence as
        // well as the presence: a surviving snake_case key means the frontend
        // validator would reject a payload the backend thinks is correct.
        for row in json["categories"].as_array().expect("categories array") {
            assert!(row["leftItems"].is_number(), "leftItems missing: {row}");
            assert!(row["itemDelta"].is_i64(), "itemDelta missing: {row}");
            assert!(row["meaningful"].is_boolean());
            assert!(
                row.get("left_items").is_none(),
                "snake_case survived: {row}"
            );
            assert!(row.get("instrument_bound").is_none());
        }
        for row in json["tools"].as_array().expect("tools array") {
            assert!(row["leftCalls"].is_number());
            assert!(row["tokenDelta"].is_i64());
            assert!(row.get("left_calls").is_none());
        }
    }

    /// A same-session diff must not load and re-fit the session twice: that
    /// is a sweep of every turn, seconds on a long session, and the reason
    /// `ct diff` makes the same optimisation at `ct-cli/src/main.rs`. The
    /// fixture is cached by the first `inspect_session` call and its file is
    /// then removed from disk, so a second, independent load for the right
    /// side would fail this test outright rather than merely run slowly.
    #[test]
    fn a_same_session_diff_does_not_reload_the_session_for_its_right_side() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        let detail = state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("first inspection populates the cache");
        let turns: Vec<u32> = detail
            .growth
            .iter()
            .filter(|point| point.prompt_tokens.is_some())
            .map(|point| point.turn)
            .collect();
        assert!(turns.len() >= 2, "need two measured turns");
        fs::remove_file(&homes.codex_session).expect("remove staged fixture after caching it");

        state
            .turn_diff(
                AgentKind::Codex,
                &codex_id,
                turns[0],
                AgentKind::Codex,
                &codex_id,
                turns[turns.len() - 1],
            )
            .expect("a same-session diff answers from the cached handle alone");
    }

    /// `homes.state()` cannot exercise `skewed` or `incomparable`: both its
    /// bindings share one flat heuristic at the same ratio (see
    /// `FixtureHomes::state`), so any diff through it is `identical`
    /// whichever two sessions are named. These two tests wire the two
    /// sessions to genuinely different instruments themselves -- the way two
    /// differently-fitted Claude Code sessions, or a Codex session against a
    /// Claude Code one, actually are on a real machine -- so the arms that
    /// were dead code before this widening are proven reachable through the
    /// desktop path, not just through `ct_application::compare`'s own tests.
    #[test]
    fn a_cross_session_diff_reports_skewed_when_two_ratios_differ() {
        let homes = FixtureHomes::new();
        let state = AppState::from_parts(
            ContextTrace::new(vec![
                AgentBinding::new(
                    Box::new(ClaudeCodeAdapter::with_home(&homes.claude_home)),
                    Box::new(HeuristicEstimator::with_ratio(2.0)),
                ),
                AgentBinding::new(
                    Box::new(CodexAdapter::with_home(&homes.codex_home)),
                    Box::new(HeuristicEstimator::with_ratio(2.5)),
                ),
            ]),
            Vec::new(),
        );
        let sessions = all_sessions(&state);
        let claude_id = session_id(&sessions, "claude-code");
        let codex_id = session_id(&sessions, "codex");
        let claude_turn = state
            .inspect_session(AgentKind::ClaudeCode, &claude_id)
            .expect("claude fixture inspects")
            .peak_turn
            .expect("fixture has prompt usage");
        let codex_turn = state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("codex fixture inspects")
            .peak_turn
            .expect("fixture has prompt usage");

        let diff = state
            .turn_diff(
                AgentKind::ClaudeCode,
                &claude_id,
                claude_turn,
                AgentKind::Codex,
                &codex_id,
                codex_turn,
            )
            .expect("two differently-ratioed sessions still bound a delta");
        let json = serde_json::to_value(&diff).expect("serialises");

        assert_eq!(json["comparability"]["kind"], "skewed");
        assert!(json["comparability"]["skew"].as_f64().unwrap() > 0.0);
        assert_eq!(json["left"]["id"], claude_id);
        assert_eq!(json["left"]["agent"], "claude-code");
        assert_eq!(json["right"]["id"], codex_id);
        assert_eq!(json["right"]["agent"], "codex");
    }

    #[test]
    fn a_cross_session_diff_reports_incomparable_across_two_kinds_of_instrument() {
        let homes = FixtureHomes::new();
        let state = AppState::from_parts(
            ContextTrace::new(vec![
                AgentBinding::new(
                    Box::new(ClaudeCodeAdapter::with_home(&homes.claude_home)),
                    Box::new(HeuristicEstimator::for_code()),
                ),
                AgentBinding::new(
                    Box::new(CodexAdapter::with_home(&homes.codex_home)),
                    Box::new(FakeTokenizer),
                ),
            ]),
            Vec::new(),
        );
        let sessions = all_sessions(&state);
        let claude_id = session_id(&sessions, "claude-code");
        let codex_id = session_id(&sessions, "codex");
        let claude_turn = state
            .inspect_session(AgentKind::ClaudeCode, &claude_id)
            .expect("claude fixture inspects")
            .peak_turn
            .expect("fixture has prompt usage");
        let codex_turn = state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("codex fixture inspects")
            .peak_turn
            .expect("fixture has prompt usage");

        let diff = state
            .turn_diff(
                AgentKind::ClaudeCode,
                &claude_id,
                claude_turn,
                AgentKind::Codex,
                &codex_id,
                codex_turn,
            )
            .expect("no comparability bound is still a typed success, not an IPC error");
        let json = serde_json::to_value(&diff).expect("serialises");

        assert_eq!(json["comparability"]["kind"], "incomparable");
        assert!(json["comparability"]["reason"].is_string());
        for row in json["categories"].as_array().expect("categories array") {
            assert_eq!(row["instrumentBound"], serde_json::Value::Null);
            assert_eq!(row["meaningful"], false);
        }
    }

    /// The three acceptance shapes CT-047 names: a Codex compaction with a
    /// real diff, a Claude Code session where the agent itself never records
    /// replacement history, and a line number that names no compaction at
    /// all.
    #[test]
    fn compaction_diff_covers_available_unsupported_and_unknown_line() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");
        let claude_id = session_id(&sessions, "claude-code");

        // -- Codex: the diff engine has evidence, and every field in it must
        // reach the frontend camelCased, including the fields *inside* each
        // disposition variant -- the enum-level `rename_all` only renames the
        // `kind` tag, not a struct variant's own fields.
        let codex_detail = state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("inspect the Codex fixture");
        let compaction = codex_detail
            .growth
            .iter()
            .find_map(|point| point.compaction.as_ref())
            .expect("the Codex fixture places one compaction on the timeline");
        let line_no = compaction.line_no;

        let diff = state
            .compaction_diff(AgentKind::Codex, &codex_id, line_no)
            .expect("the Codex fixture's compaction diff is available");
        let (turn, reported_line_no, items) = match diff {
            CompactionDiffSummary::Available {
                turn,
                line_no,
                items,
            } => (turn, line_no, items),
            _ => panic!("the Codex fixture's one compaction has recorded replacement history"),
        };
        assert_eq!(reported_line_no, line_no);
        assert!(turn.is_some());

        // The committed fixture's one compaction is documented (BACKLOG.md
        // CT-047) as 4 dropped, 1 preserved, 1 replacement-only.
        let dropped = items
            .iter()
            .filter(|item| {
                matches!(
                    item.disposition,
                    CompactionDispositionSummary::Dropped { .. }
                )
            })
            .count();
        let preserved = items
            .iter()
            .filter(|item| {
                matches!(
                    item.disposition,
                    CompactionDispositionSummary::Preserved { .. }
                )
            })
            .count();
        let added = items
            .iter()
            .filter(|item| {
                matches!(
                    item.disposition,
                    CompactionDispositionSummary::AddedByReplacement { .. }
                )
            })
            .count();
        assert_eq!((dropped, preserved, added), (4, 1, 1));

        // The fixture-backed harness wires the heuristic estimator for both
        // bindings (see `FixtureHomes::state`), not tiktoken, so
        // `TokenCount::is_trustworthy` never passes here -- `text_tokens`
        // must stay present-but-null, never coerced to a number that was not
        // measured.
        assert!(items.iter().all(|item| item.text_tokens.is_none()));
        assert!(items
            .iter()
            .all(|item| matches!(item.confidence, Confidence::Derived)));

        let json = serde_json::to_value(&items).expect("diff items serialize for IPC");
        let preserved_json = json
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["disposition"]["kind"] == "preserved")
            .expect("one item is preserved");
        assert!(preserved_json["itemType"].is_string());
        assert!(preserved_json.get("item_type").is_none());
        assert!(preserved_json["normalizedJsonBytes"].is_number());
        assert!(preserved_json.get("normalized_json_bytes").is_none());
        assert!(preserved_json["disposition"]["historyIndex"].is_number());
        assert!(preserved_json["disposition"]["replacementIndex"].is_number());
        assert!(preserved_json["disposition"].get("history_index").is_none());
        assert!(preserved_json["disposition"]
            .get("replacement_index")
            .is_none());
        assert_eq!(preserved_json["textTokens"], serde_json::Value::Null);

        // -- Claude Code: the adapter never overrides `compaction_diffs`, so
        // every one of its sessions hits the agent-level refusal before any
        // single compaction's evidence is considered -- regardless of
        // whether this particular session recorded a compaction marker of
        // its own kind. Reusing the Codex fixture's line number is
        // deliberate: it proves the refusal is unconditional, not merely "no
        // compaction found at that line".
        let claude_diff = state
            .compaction_diff(AgentKind::ClaudeCode, &claude_id, line_no)
            .expect("Claude Code's refusal is a typed success, not an IPC error");
        match &claude_diff {
            CompactionDiffSummary::Unsupported { detail } => {
                assert!(
                    detail.contains("claude-code") || detail.contains("Claude"),
                    "the refusal should name the agent, not just say something failed: {detail}"
                );
                assert!(
                    detail.contains("replacement history"),
                    "the refusal should explain the evidence limit, not merely say 'unsupported': {detail}"
                );
            }
            _ => panic!("Claude Code records no replacement history at all"),
        }
        let claude_json = serde_json::to_value(&claude_diff).expect("serializes for IPC");
        assert_eq!(claude_json["status"], "unsupported");
        assert!(claude_json["detail"].is_string());

        // -- A line number naming no compaction at all is a genuine error,
        // not a third typed outcome: unlike the two cases above, there is no
        // "fact learned" here to encode, only a request that cannot be
        // answered.
        let error = match state.compaction_diff(AgentKind::Codex, &codex_id, 999_999) {
            Err(error) => error,
            Ok(_) => panic!("line 999999 names no compaction in the fixture"),
        };
        assert!(error.contains("no compaction recorded"));
    }

    /// This view is built on a fitted ratio, and Codex is never fitted one --
    /// the refusal fires before `cached_session` is even consulted, so it
    /// must hold for every Codex session, not just ones short on growth.
    #[test]
    fn a_codex_sessions_residual_report_is_agent_not_fitted() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");

        let report = state
            .residual(AgentKind::Codex, &codex_id)
            .expect("the agent refusal is a typed success, not an IPC error");
        let json = serde_json::to_value(&report).expect("serializes for IPC");
        assert_eq!(json["kind"], "agentNotFitted");
        assert_eq!(json["agent"], "codex");
    }

    /// The fixtures are too short to fit a ratio (`derive_ratio` wants five
    /// growing turn pairs; the committed fixtures hold two turns), so the
    /// `fitted` shape is exercised directly rather than through a session
    /// that cannot reach it. What this pins is the JSON contract: every field
    /// camelCased, including inside `points` and `steps`, and no snake_case
    /// sibling left over from the enum-level `rename_all` not reaching a
    /// struct variant's own fields -- the mistake this file documents having
    /// made once already on `ComparabilitySummary`.
    #[test]
    fn a_fitted_residual_report_camelcases_every_field_including_nested_ones() {
        let report = ResidualReport::Fitted {
            chars_per_token: 3.5,
            pairs_used: 12,
            dispersion: 1.1,
            unlogged_overhead: Some(42_000),
            turns_measured: 20,
            over_counted_turns: 2,
            step_threshold: RESIDUAL_STEP_THRESHOLD,
            prompt_confidence: Confidence::Observed,
            remainder_confidence: Confidence::Derived,
            points: vec![
                ResidualPointSummary::from(&ResidualPoint {
                    turn: 5,
                    prompt_tokens: 100_000,
                    accounted: 60_000,
                    unlogged: Some(40_000),
                    items: 12,
                }),
                ResidualPointSummary::from(&ResidualPoint {
                    turn: 6,
                    prompt_tokens: 30_000,
                    accounted: 45_000,
                    unlogged: None,
                    items: 14,
                }),
            ],
            steps: vec![ResidualStepSummary {
                turn: 21,
                from: 30_000,
                to: 42_000,
                growth: 12_000,
                near_compaction: true,
            }],
        };

        let json = serde_json::to_value(&report).expect("serializes for IPC");
        assert_eq!(json["kind"], "fitted");

        assert!(json["charsPerToken"].is_number());
        assert!(json["pairsUsed"].is_number());
        assert!(json["unloggedOverhead"].is_number());
        assert!(json["turnsMeasured"].is_number());
        assert!(json["overCountedTurns"].is_number());
        assert!(json["stepThreshold"].is_number());
        assert_eq!(json["promptConfidence"], "observed");
        assert_eq!(json["remainderConfidence"], "derived");
        assert!(json.get("chars_per_token").is_none());
        assert!(json.get("pairs_used").is_none());
        assert!(json.get("unlogged_overhead").is_none());
        assert!(json.get("turns_measured").is_none());
        assert!(json.get("over_counted_turns").is_none());
        assert!(json.get("step_threshold").is_none());
        assert!(json.get("prompt_confidence").is_none());
        assert!(json.get("remainder_confidence").is_none());

        let point = &json["points"][0];
        assert!(point["promptTokens"].is_number());
        assert!(point["unlogged"].is_number());
        assert!(point.get("prompt_tokens").is_none());
        let over_counted_point = &json["points"][1];
        assert_eq!(over_counted_point["unlogged"], serde_json::Value::Null);

        let step = &json["steps"][0];
        assert!(step["nearCompaction"].is_boolean());
        assert!(step["growth"].is_i64());
        assert!(step.get("near_compaction").is_none());
    }

    /// The discrimination CT-072 calls out as the whole risk: a session can
    /// fit a ratio and still have nothing plottable, when reconstruction
    /// over-counts on every turn. Pinned as a pure-function test rather than
    /// through `AppState::residual`, because the committed fixtures are too
    /// short to fit a ratio at all, let alone an over-counting one -- and a
    /// pure classifier over `&[ResidualPoint]` is testable without either.
    #[test]
    fn a_series_with_no_known_remainder_is_not_reported_as_fitted() {
        let point = |turn: u32, unlogged: Option<u32>| ResidualPoint {
            turn,
            prompt_tokens: 100_000,
            accounted: 100_000,
            unlogged,
            items: 5,
        };

        let all_over_counted = vec![point(1, None), point(2, None), point(3, None)];
        assert!(
            !series_has_a_measurable_remainder(&all_over_counted),
            "every turn over-counted must not read as fitted"
        );

        let mixed = vec![point(1, None), point(2, Some(40_000)), point(3, None)];
        assert!(
            series_has_a_measurable_remainder(&mixed),
            "one known remainder is enough to fit, with the rest counted as over_counted_turns"
        );

        assert!(
            !series_has_a_measurable_remainder(&[]),
            "nothing can be drawn from zero points either"
        );
    }

    // ---- archive DTOs ---------------------------------------------------
    //
    // `AppState::archive_session`, `::archived_sessions` and
    // `::verify_archived` each resolve the archive root through
    // `ct_runtime::archive_store()`, which is not test-injectable: it always
    // resolves the real per-user (or `CONTEXTTRACE_ARCHIVE`-overridden)
    // directory. Exercising those three methods end-to-end here would mean
    // either writing into a real machine's archive during `cargo test` or
    // mutating that process-wide environment variable across a parallel test
    // run -- both worse than the coverage gained. So these tests go around
    // that seam and through `self.app` (a `ContextTrace`, reachable from a
    // child module the ordinary way private fields are) with a
    // `FileArchiveStore` pointed at a throwaway directory, which is exactly
    // what `archive_session_in_agent`/`verify_archived_in_agent` themselves
    // are -- the real logic under test is the DTO mapping this file owns,
    // not the seam `ct_runtime` already tests on its own.

    #[test]
    fn archive_entry_summary_mirrors_the_domain_entry_camelcased() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");
        let store = ct_adapters::FileArchiveStore::at(homes.root.join("archive-under-test"));

        let entry = state
            .app
            .archive_session_in_agent(AgentKind::Codex, &codex_id, &store, RedactionMode::Redacted)
            .expect("a discovered session archives");

        let summary = ArchiveEntrySummary::from(entry);
        let json = serde_json::to_value(&summary).expect("serializes for IPC");
        assert_eq!(json["id"], codex_id);
        assert_eq!(json["agent"], "codex");
        assert_eq!(json["redaction"], "redacted");
        assert!(json["records"].is_number());
        assert!(json["sourceBytes"].is_number());
        assert!(json["archivedBytes"].is_number());
        assert!(json["archivedAt"].is_string());
        assert!(json["differsFromSource"].is_boolean());
        // `rename_all` is on the DTO struct, not inherited from anywhere
        // else -- assert the snake_case sibling is truly gone, not merely
        // that the camelCase key is present.
        assert!(json.get("source_bytes").is_none());
        assert!(json.get("archived_bytes").is_none());
        assert!(json.get("differs_from_source").is_none());
    }

    #[test]
    fn archive_entries_translate_into_dtos_in_the_same_order_they_arrived() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = all_sessions(&state);
        let codex_id = session_id(&sessions, "codex");
        let claude_id = session_id(&sessions, "claude-code");
        let store = ct_adapters::FileArchiveStore::at(homes.root.join("archive-under-test"));

        state
            .app
            .archive_session_in_agent(AgentKind::Codex, &codex_id, &store, RedactionMode::Redacted)
            .expect("codex fixture archives");
        state
            .app
            .archive_session_in_agent(
                AgentKind::ClaudeCode,
                &claude_id,
                &store,
                RedactionMode::Redacted,
            )
            .expect("claude fixture archives");

        // `ContextTrace::archived_sessions` orders most-recently-archived
        // first and is tested on its own in `ct_application::archive`; this
        // only pins that turning its entries into `ArchiveEntrySummary` does
        // not re-sort them, the property `ArchiveHolding`'s own doc comment
        // promises.
        let entries = state
            .app
            .archived_sessions(&store)
            .expect("list archived sessions");
        let domain_order: Vec<String> =
            entries.iter().map(|entry| entry.id().to_string()).collect();
        let dto_order: Vec<String> = entries
            .into_iter()
            .map(ArchiveEntrySummary::from)
            .map(|summary| summary.id)
            .collect();
        // Whichever order the domain settled on -- ties on `archived_at` are
        // possible at whatever clock resolution a given machine has, and
        // `ContextTrace::archived_sessions` already covers the ordering rule
        // itself -- the DTO layer must reproduce it exactly rather than
        // impose one of its own.
        assert_eq!(domain_order, dto_order);
        assert_eq!(
            domain_order
                .iter()
                .collect::<std::collections::BTreeSet<_>>(),
            [&codex_id, &claude_id].into_iter().collect(),
            "both archived sessions are present"
        );
    }

    #[test]
    fn archive_verification_reads_copy_is_sound_and_rebuildable_off_the_domain_value() {
        // A vanished source with a matching digest: the case the whole
        // feature exists for. The copy is sound (nothing to rebuild it from
        // says otherwise) but not rebuildable (there is nothing left to
        // re-read), and both figures must come from `ArchiveIntegrity`'s own
        // methods rather than be re-derived here.
        let integrity = ArchiveIntegrity::SourceGone {
            archive_matches_digest: true,
        };
        let verification = ArchiveVerification::from(integrity);
        assert!(verification.copy_is_sound);
        assert!(!verification.rebuildable);

        let json = serde_json::to_value(&verification).expect("serializes for IPC");
        assert_eq!(json["integrity"]["kind"], "sourceGone");
        assert_eq!(json["integrity"]["archiveMatchesDigest"], true);
        assert_eq!(json["copyIsSound"], true);
        assert_eq!(json["rebuildable"], false);
        assert!(json.get("copy_is_sound").is_none());
        assert!(json.get("archive_matches_digest").is_none());
    }

    #[test]
    fn archive_damaged_is_the_one_outcome_the_copy_is_not_sound_under() {
        let integrity = ArchiveIntegrity::ArchiveDamaged {
            recorded_digest: "a".into(),
            current_digest: "b".into(),
        };
        let json =
            serde_json::to_value(ArchiveVerification::from(integrity)).expect("serializes for IPC");
        assert_eq!(json["integrity"]["kind"], "archiveDamaged");
        assert_eq!(json["integrity"]["recordedDigest"], "a");
        assert_eq!(json["integrity"]["currentDigest"], "b");
        assert!(json["integrity"].get("recorded_digest").is_none());
        assert_eq!(json["copyIsSound"], false);
        assert_eq!(json["rebuildable"], true);
    }

    #[test]
    fn archive_intact_needs_no_rebuild_and_every_field_camelcases() {
        let json = serde_json::to_value(ArchiveVerification::from(ArchiveIntegrity::Intact))
            .expect("serializes for IPC");
        assert_eq!(json["integrity"]["kind"], "intact");
        assert_eq!(json["copyIsSound"], true);
        assert_eq!(json["rebuildable"], false);

        let source_changed = ArchiveIntegrity::SourceChanged {
            recorded_digest: "old".into(),
            current_digest: "new".into(),
            recorded_bytes: 10,
            current_bytes: 20,
        };
        let json = serde_json::to_value(ArchiveVerification::from(source_changed))
            .expect("serializes for IPC");
        assert_eq!(json["integrity"]["kind"], "sourceChanged");
        assert_eq!(json["integrity"]["recordedBytes"], 10);
        assert_eq!(json["integrity"]["currentBytes"], 20);
        assert!(json["integrity"].get("recorded_bytes").is_none());
        // A source that merely continued is still a sound copy of an earlier
        // state, and re-ingesting is offered rather than refused.
        assert_eq!(json["copyIsSound"], true);
        assert_eq!(json["rebuildable"], true);
    }

    // ---- export ----------------------------------------------------------

    #[test]
    fn an_export_writes_every_record_as_a_line_that_parses_on_its_own() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let id = session_id(&all_sessions(&state), "codex");
        let dest = homes.root.join("exports");

        let outcome = state
            .export_session_into(&dest, AgentKind::Codex, &id, false)
            .expect("the fixture exports");

        let written = fs::read_to_string(&outcome.path).expect("the export is on disk");
        let lines: Vec<&str> = written.lines().collect();
        assert_eq!(lines.len() as u64, outcome.records);
        assert_eq!(written.len() as u64, outcome.bytes);
        // NDJSON's whole promise is that a consumer can read one line at a
        // time. A file that only parses as a whole would still look right.
        for line in &lines {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|error| panic!("every exported line parses alone: {error}"));
        }
        assert_eq!(outcome.redaction, "none");
        assert_eq!(outcome.redactions, 0);
    }

    #[test]
    fn an_export_leaves_no_pending_file_behind() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let id = session_id(&all_sessions(&state), "codex");
        let dest = homes.root.join("exports");

        let outcome = state
            .export_session_into(&dest, AgentKind::Codex, &id, false)
            .expect("the fixture exports");

        // The rename is what makes a complete export distinguishable from an
        // interrupted one; a leftover `.pending` beside it would mean a reader
        // has two files to choose between and no rule for which is whole.
        let siblings: Vec<String> = fs::read_dir(dest.join("codex"))
            .expect("the agent directory was created")
            .map(|entry| {
                entry
                    .expect("readable")
                    .file_name()
                    .to_string_lossy()
                    .into()
            })
            .collect();
        assert_eq!(siblings.len(), 1, "found {siblings:?}");
        assert!(outcome.path.ends_with(".ndjson"));
    }

    #[test]
    fn an_export_asked_to_redact_says_so_even_when_it_finds_nothing() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let id = session_id(&all_sessions(&state), "codex");
        let dest = homes.root.join("exports");

        let outcome = state
            .export_session_into(&dest, AgentKind::Codex, &id, true)
            .expect("the fixture exports");

        // The two fields answer different questions. This fixture carries no
        // credentials, so the count is zero -- which must not be reportable as
        // "nothing was redacted" in a file that was never scanned.
        assert_eq!(outcome.redaction, "secrets");
        assert_eq!(outcome.redactions, 0);
    }

    #[test]
    fn re_exporting_replaces_the_previous_file_rather_than_appending_to_it() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let id = session_id(&all_sessions(&state), "codex");
        let dest = homes.root.join("exports");

        let first = state
            .export_session_into(&dest, AgentKind::Codex, &id, false)
            .expect("first export");
        let second = state
            .export_session_into(&dest, AgentKind::Codex, &id, false)
            .expect("second export");

        assert_eq!(first.path, second.path);
        assert_eq!(first.bytes, second.bytes);
        let on_disk = fs::metadata(&second.path).expect("still one file").len();
        assert_eq!(on_disk, second.bytes);
    }

    #[test]
    fn an_export_is_named_by_the_same_rule_the_archive_names_its_copies_by() {
        // Both write paths put a session id in a filename, and a user reading
        // one subdirectory beside the other should not find the same session
        // under two different stems.
        let id = ct_domain::SessionId::new("a/b..c").expect("non-blank");
        assert_eq!(id.file_stem().as_deref(), Some("a%2Fb..c"));
        assert!(ct_domain::SessionId::new("..")
            .expect("non-blank")
            .file_stem()
            .is_none());
    }
}
