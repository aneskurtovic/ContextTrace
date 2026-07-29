use ct_application::{timeline, ContextTrace, SessionFilter};
use ct_domain::{
    AgentKind, CategoryBreakdown, Confidence, ContextSource, SessionDescriptor, TurnNumber,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Mutex;

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
        Self {
            app: runtime.app,
            warnings: runtime.warnings,
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
    StartupSummary {
        roots: state
            .app
            .roots()
            .into_iter()
            .map(|(agent, paths)| RootSummary {
                agent: agent.to_string(),
                paths,
            })
            .collect(),
        warnings: state.warnings.clone(),
    }
}

#[tauri::command]
pub fn list_sessions(
    agent: Option<String>,
    project: Option<String>,
    limit: Option<usize>,
    state: tauri::State<'_, AppState>,
) -> Result<Vec<SessionSummary>, String> {
    state
        .sessions
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
        project: project.filter(|value| !value.trim().is_empty()),
        since: None,
        limit: Some(limit.unwrap_or(200).min(1_000)),
    };
    Ok(state
        .app
        .list_sessions(&filter)
        .into_iter()
        .map(SessionSummary::from)
        .collect())
}

#[tauri::command]
pub fn inspect_session(
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<SessionDetail, String> {
    state.with_session(&id, |cached| {
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

#[tauri::command]
pub fn get_context(
    id: String,
    turn: Option<u32>,
    state: tauri::State<'_, AppState>,
) -> Result<ContextDetail, String> {
    state.with_session(&id, |cached| {
        let turn = match turn {
            Some(turn) => TurnNumber::new(turn).map_err(|error| error.to_string())?,
            None => state
                .app
                .peak_turn(&cached.session)
                .ok_or_else(|| "this session has no turn with prompt usage".to_string())?,
        };
        let estimator = cached.chars_per_token.map(ct_runtime::heuristic_estimator);
        let snapshot = match estimator.as_ref() {
            Some(estimator) => {
                state
                    .app
                    .snapshot_with(&cached.session, cached.binding, turn, estimator)
            }
            None => state.app.snapshot(&cached.session, cached.binding, turn),
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

fn format_source(source: &ContextSource) -> String {
    source.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
