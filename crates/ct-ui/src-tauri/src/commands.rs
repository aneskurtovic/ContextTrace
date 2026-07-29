use ct_application::{timeline, ContextTrace, SessionFilter};
use ct_domain::{
    AgentKind, CategoryBreakdown, Confidence, ContextSource, SessionDescriptor, TurnNumber,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

const DEFAULT_SESSION_PAGE_SIZE: usize = 200;
const MAX_SESSION_PAGE_SIZE: usize = 1_000;

pub struct AppState {
    app: ContextTrace,
    warnings: Vec<String>,
    sessions: Mutex<HashMap<String, CachedSession>>,
}

struct CachedSession {
    session: ct_domain::AgentSession,
    descriptor: SessionDescriptor,
    binding: usize,
    chars_per_token: Option<f32>,
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
            sessions: Mutex::new(HashMap::new()),
        }
    }

    fn with_session<T>(
        &self,
        id: &str,
        use_session: impl FnOnce(&CachedSession) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut sessions = self
            .sessions
            .lock()
            .map_err(|_| "the in-memory session cache is unavailable".to_string())?;
        if !sessions.contains_key(id) {
            let (session, resolved) = self.app.load(id).map_err(|error| error.to_string())?;
            let (_, ratio) = ct_runtime::calibrate_session(&self.app, &session, resolved.binding);
            sessions.insert(
                id.to_string(),
                CachedSession {
                    session,
                    descriptor: resolved.descriptor,
                    binding: resolved.binding,
                    chars_per_token: ratio.map(|ratio| ratio.chars_per_token),
                },
            );
        }
        use_session(
            sessions
                .get(id)
                .expect("a session was inserted immediately above"),
        )
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

    fn list_sessions(
        &self,
        agent: Option<String>,
        project: Option<String>,
        limit: Option<usize>,
    ) -> Result<Vec<SessionSummary>, String> {
        Ok(self
            .search_sessions(agent, project, Some(0), limit)?
            .sessions)
    }

    /// Search the complete local catalog before taking a page.
    ///
    /// The older list command remains for existing desktop clients, but it
    /// deliberately has no way to say whether a caller's fixed limit hid a
    /// result. New clients should use this page contract so a narrow query can
    /// find a session that falls after an unfiltered first page.
    fn search_sessions(
        &self,
        agent: Option<String>,
        query: Option<String>,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<SessionPage, String> {
        self.sessions
            .lock()
            .map_err(|_| "the in-memory session cache is unavailable".to_string())?
            .clear();
        let parsed_agent = match agent.as_deref() {
            Some(agent) => Some(
                AgentKind::parse(agent)
                    .ok_or_else(|| format!("unknown agent '{agent}'; use claude-code or codex"))?,
            ),
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

    fn inspect_session(&self, id: &str) -> Result<SessionDetail, String> {
        self.with_session(id, |cached| {
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
        })
    }

    fn context(&self, id: &str, turn: Option<u32>) -> Result<ContextDetail, String> {
        self.with_session(id, |cached| {
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

#[tauri::command]
pub fn get_startup(state: tauri::State<'_, AppState>) -> StartupSummary {
    state.startup()
}

#[tauri::command]
pub fn list_sessions(
    agent: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SessionSummary>, String> {
    state.list_sessions(agent, project, limit)
}

/// Search session metadata on the backend and return explicit paging facts.
///
/// Kept separate from `list_sessions` for IPC compatibility with already
/// released clients that expect an array response.
#[tauri::command]
pub fn search_sessions(
    agent: Option<String>,
    query: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<SessionPage, String> {
    state.search_sessions(agent, query, offset, limit)
}

#[tauri::command]
pub fn inspect_session(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<SessionDetail, String> {
    state.inspect_session(&id)
}

#[tauri::command]
pub fn get_context(
    id: String,
    turn: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<ContextDetail, String> {
    state.context(&id, turn)
}

fn format_source(source: &ContextSource) -> String {
    source.to_string()
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

    fn catalog_descriptor(index: usize, project: &str) -> SessionDescriptor {
        SessionDescriptor {
            id: ct_domain::SessionId::new(format!("catalog-{index:04}")).unwrap(),
            agent: AgentKind::Codex,
            path: format!("C:/catalog/session-{index:04}.jsonl"),
            size_bytes: 1,
            project: Some(project.to_string()),
            started_at: None,
            last_activity: None,
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
        let detail = state.inspect_session(id).expect("inspect fixture session");
        assert_eq!(detail.session.agent, agent);
        assert_eq!(detail.model.as_deref(), Some(model));
        assert!(detail.turn_count > 0);
        assert!(detail.event_count > 0);
        let peak_turn = detail.peak_turn.expect("fixture has prompt usage");

        let context = state
            .context(id, None)
            .expect("load context for the peak turn");
        assert_eq!(context.turn, peak_turn);
        assert_eq!(context.model.as_deref(), Some(model));
        assert!(context.total_tokens > 0);
        assert!(!context.categories.is_empty());
        assert!(!context.contributors.is_empty());

        let json = serde_json::to_value(context).expect("context detail serializes for IPC");
        assert!(json["totalTokens"].is_number());
        assert!(json["residualIsMeaningful"].is_boolean());
        assert!(json.get("total_tokens").is_none());
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
        };

        let summary = SessionSummary::from(descriptor);
        assert_eq!(summary.id, "abc123");
        assert_eq!(summary.agent, "codex");
        assert_eq!(summary.size_bytes, 42);
    }

    #[test]
    fn fixture_backed_list_inspect_and_context_contract_covers_both_agents() {
        let homes = FixtureHomes::new();
        let state = homes.state();

        let startup = state.startup();
        assert_eq!(startup.roots.len(), 2);
        assert_eq!(startup.warnings, ["synthetic fixture runtime"]);

        let sessions = state
            .list_sessions(None, None, None)
            .expect("list committed synthetic fixtures");
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
            .search_sessions(Some("codex".into()), None, Some(0), Some(500))
            .expect("list the first page");
        assert_eq!(first_page.total, 501);
        assert_eq!(first_page.sessions.len(), 500);
        assert!(
            first_page.has_more,
            "the first page must not impersonate the catalog"
        );

        let targeted = state
            .search_sessions(None, Some("  TARGETED older  ".into()), Some(0), Some(50))
            .expect("search the complete catalog before paging");
        assert_eq!(targeted.total, 1);
        assert_eq!(targeted.sessions.len(), 1);
        assert!(!targeted.has_more);
        assert_eq!(targeted.sessions[0].id, "catalog-0500");

        let final_page = state
            .search_sessions(None, None, Some(500), Some(50))
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
    fn list_refreshes_the_session_cache_before_a_follow_up_inspection() {
        let homes = FixtureHomes::new();
        let state = homes.state();
        let sessions = state
            .list_sessions(None, None, None)
            .expect("list committed synthetic fixtures");
        let codex_id = session_id(&sessions, "codex");

        state
            .inspect_session(&codex_id)
            .expect("first inspection populates the cache");
        fs::remove_file(&homes.codex_session).expect("remove staged fixture after caching it");
        state
            .inspect_session(&codex_id)
            .expect("cached inspection does not reread a session until refresh");

        let refreshed = state
            .list_sessions(None, None, None)
            .expect("refresh the session listing");
        assert_eq!(refreshed.len(), 1);
        assert_eq!(refreshed[0].agent, "claude-code");
        let error = match state.inspect_session(&codex_id) {
            Err(error) => error,
            Ok(_) => panic!("refresh evicts sessions that have disappeared from disk"),
        };
        assert!(error.contains("no session matching"));
    }

    #[test]
    fn service_reports_actionable_invalid_agent_session_and_turn_errors() {
        let homes = FixtureHomes::new();
        let state = homes.state();

        let error = match state.list_sessions(Some("cursor".to_string()), None, None) {
            Err(error) => error,
            Ok(_) => panic!("unsupported agents are rejected before discovery"),
        };
        assert_eq!(error, "unknown agent 'cursor'; use claude-code or codex");

        let error = match state.inspect_session("does-not-exist") {
            Err(error) => error,
            Ok(_) => panic!("missing sessions are reported to the desktop client"),
        };
        assert_eq!(error, "no session matching 'does-not-exist'");

        let sessions = state
            .list_sessions(Some("codex".to_string()), None, None)
            .expect("list Codex fixture");
        let codex_id = session_id(&sessions, "codex");
        let error = match state.context(&codex_id, Some(999)) {
            Err(error) => error,
            Ok(_) => panic!("out-of-range turns are not silently substituted"),
        };
        assert_eq!(
            error,
            "turn 999 is out of range; this session has 2 turn(s)"
        );
    }
}
