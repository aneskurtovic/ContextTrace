use ct_application::{timeline, ContextTrace, Departure, LifecycleSweep, SessionFilter};
use ct_domain::model::context::unmeasured_content_items;
use ct_domain::{
    AgentKind, CategoryBreakdown, Confidence, ContextItemId, ContextSource, SessionDescriptor,
    ThreadRole, TurnNumber,
};
use serde::Serialize;
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

const DEFAULT_SESSION_PAGE_SIZE: usize = 200;
const MAX_SESSION_PAGE_SIZE: usize = 1_000;

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
    sessions: Mutex<BoundedCache<SessionKey, Arc<CachedSession>>>,
    lifecycles: Mutex<BoundedCache<SessionKey, Arc<LifecycleSweep>>>,
}

struct CachedSession {
    session: ct_domain::AgentSession,
    descriptor: SessionDescriptor,
    binding: usize,
    chars_per_token: Option<f32>,
    content_analyzed: bool,
}

impl AppState {
    pub fn new() -> Self {
        let runtime = ct_runtime::build();
        Self::from_parts(runtime.app, runtime.warnings)
    }

    fn from_parts(app: ContextTrace, warnings: Vec<String>) -> Self {
        Self {
            app,
            warnings,
            sessions: Mutex::new(BoundedCache::new(SESSION_CACHE_CAPACITY)),
            lifecycles: Mutex::new(BoundedCache::new(LIFECYCLE_CACHE_CAPACITY)),
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

        let (session, resolved) = if analyzed {
            self.app.load_with_content_analysis_in_agent(agent, id)
        } else {
            self.app.load_in_agent(agent, id)
        }
        .map_err(|error| error.to_string())?;
        debug_assert_eq!(resolved.descriptor.agent, agent);
        let (_, ratio) = ct_runtime::calibrate_session(&self.app, &session, resolved.binding);
        let cached = Arc::new(CachedSession {
            session,
            descriptor: resolved.descriptor,
            binding: resolved.binding,
            chars_per_token: ratio.map(|ratio| ratio.chars_per_token),
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
        }
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
    fn search_sessions(
        &self,
        agent: Option<String>,
        query: Option<String>,
        offset: Option<usize>,
        limit: Option<usize>,
        refresh: Option<bool>,
    ) -> Result<SessionPage, String> {
        if refresh.unwrap_or(false) {
            self.sessions().clear();
            self.lifecycles().clear();
        }
        let parsed_agent = match agent.as_deref() {
            Some(agent) => Some(parse_agent(agent)?),
            None => None,
        };
        let filter = SessionFilter {
            agent: parsed_agent,
            // Search below intentionally includes stable ids and local source
            // paths as well as projects. Do not pre-filter by `project` here:
            // doing so would make an id/path search silently incomplete.
            project: None,
            since: None,
            limit: None,
        };
        let query = query
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let mut sessions: Vec<SessionSummary> = self
            .app
            .list_sessions(&filter)
            .into_iter()
            .filter(|descriptor| session_matches_query(descriptor, query.as_deref()))
            .map(SessionSummary::from)
            .collect();
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

    fn inspect_session(&self, agent: AgentKind, id: &str) -> Result<SessionDetail, String> {
        let cached = self.cached_session(agent, id, false)?;
        let session = &cached.session;
        let growth = timeline(session);
        let peak_turn = session.peak_turn().map(|turn| turn.get());
        let metadata = session.metadata();

        Ok(SessionDetail {
            session: cached.descriptor.clone().into(),
            model: metadata.model.clone(),
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
                    }),
                })
                .collect(),
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
        let estimator = cached.chars_per_token.map(ct_runtime::heuristic_estimator);
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
        let estimator = cached.chars_per_token.map(ct_runtime::heuristic_estimator);
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
}

/// A session's place in its thread group, for the desktop's session list.
///
/// Mirrors [`ThreadRole`] rather than flattening it into two nullable fields:
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

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSummary {
    id: String,
    agent: String,
    path: String,
    size_bytes: u64,
    project: Option<String>,
    started_at: Option<String>,
    last_activity: Option<String>,
    thread_role: ThreadRoleSummary,
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

impl From<SessionDescriptor> for SessionSummary {
    fn from(value: SessionDescriptor) -> Self {
        Self {
            id: value.id.to_string(),
            agent: value.agent.to_string(),
            path: value.path,
            size_bytes: value.size_bytes,
            project: value.project,
            started_at: value.started_at.map(|time| time.to_rfc3339()),
            last_activity: value.last_activity.map(|time| time.to_rfc3339()),
            thread_role: value.thread_role.into(),
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompactionSummary {
    turn: Option<u32>,
    reclaimed: Option<u32>,
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

#[tauri::command]
pub fn get_startup(state: tauri::State<'_, AppState>) -> StartupSummary {
    state.startup()
}

/// Search session metadata on the backend and return explicit paging facts.
#[tauri::command]
pub fn search_sessions(
    agent: Option<String>,
    query: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    refresh: Option<bool>,
    state: tauri::State<'_, AppState>,
) -> Result<SessionPage, String> {
    state.search_sessions(agent, query, offset, limit, refresh)
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

fn format_source(source: &ContextSource) -> String {
    source.to_string()
}

fn parse_agent(agent: &str) -> Result<AgentKind, String> {
    AgentKind::parse(agent)
        .ok_or_else(|| format!("unknown agent '{agent}'; use claude-code or codex"))
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
    let matches = [
        descriptor.id.as_str(),
        descriptor.project.as_deref().unwrap_or_default(),
        descriptor.path.as_str(),
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
            .search_sessions(None, None, Some(0), Some(MAX_SESSION_PAGE_SIZE), None)
            .expect("list committed synthetic fixtures")
            .sessions
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
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        }
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

        let json = serde_json::to_value(&context).expect("context detail serializes for IPC");
        assert!(json["totalTokens"].is_number());
        assert!(json["residualIsMeaningful"].is_boolean());
        assert!(json.get("total_tokens").is_none());

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

    #[test]
    fn session_summary_uses_stable_ui_strings() {
        let descriptor = SessionDescriptor {
            id: ct_domain::SessionId::new("abc123").unwrap(),
            agent: AgentKind::Codex,
            path: "session.jsonl".to_string(),
            size_bytes: 42,
            project: Some("ContextTrace".to_string()),
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
            .search_sessions(Some("codex".into()), None, Some(0), Some(500), None)
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
            .search_sessions(None, None, Some(500), Some(50), None)
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
            .search_sessions(None, None, Some(1), Some(1), None)
            .expect("page through the catalog without asking for a refresh");
        state
            .inspect_session(AgentKind::Codex, &codex_id)
            .expect("pagination does not evict a session already parsed");

        // Nor does an ordinary offset-0 search issued without `refresh`, e.g.
        // a query or filter change.
        state
            .search_sessions(None, Some("".into()), Some(0), Some(50), None)
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
            .search_sessions(None, None, Some(0), None, Some(true))
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
    }

    #[test]
    fn agent_and_id_together_identify_a_session_when_ids_collide_across_agents() {
        let (state, _homes, id) = colliding_id_state();

        let page = state
            .search_sessions(None, None, Some(0), None, None)
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

        let error = match state.search_sessions(Some("cursor".to_string()), None, None, None, None)
        {
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
            .search_sessions(Some("codex".to_string()), None, None, None, None)
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
}
