//! Explicit imports of stdout captures. These schemas describe presentation
//! events, not persisted request histories. Never feed them to replay/DAG ACLs.
use crate::jsonl;
use crate::walk::{find_files, has_extension};
use ct_domain::ports::{AgentAdapter, PortError, PortResult, ReconstructedContext, TokenEstimator};
use ct_domain::{
    AgentKind, AgentSession, Event, EventId, EventKind, FileId, MessageRole, SessionDescriptor,
    SessionId, SessionMetadata, SourceRef, ThreadRole, TurnNumber,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamSurface {
    CodexExec,
    CodexAppServer,
    ClaudeStream,
}
impl StreamSurface {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "codex-exec" => Some(Self::CodexExec),
            "codex-app-server" => Some(Self::CodexAppServer),
            "claude-stream" => Some(Self::ClaudeStream),
            _ => None,
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::CodexExec => "codex-exec",
            Self::CodexAppServer => "codex-app-server",
            Self::ClaudeStream => "claude-stream",
        }
    }
    fn agent(self) -> AgentKind {
        if self == Self::ClaudeStream {
            AgentKind::ClaudeCode
        } else {
            AgentKind::Codex
        }
    }
}

#[derive(Debug, Serialize)]
pub struct StreamUsage {
    pub source: SourceRef,
    /// Aggregates are never presented as one request's context size.
    pub scope: String,
    pub reported: Value,
}
#[derive(Debug, Serialize)]
pub struct StreamImport {
    pub surface: String,
    pub producer_version: String,
    pub session: AgentSession,
    pub usage: Vec<StreamUsage>,
    pub limitations: Vec<String>,
}

/// Discovery is opt-in, limited to the explicitly supplied capture directory.
/// No producer session root is searched for stdout-shaped files.
pub struct StreamAdapter {
    root: PathBuf,
    surface: StreamSurface,
    version: String,
}
impl StreamAdapter {
    pub fn new(
        root: impl Into<PathBuf>,
        surface: StreamSurface,
        version: impl Into<String>,
    ) -> Self {
        Self {
            root: root.into(),
            surface,
            version: version.into(),
        }
    }
    pub fn import(&self, path: &Path) -> PortResult<StreamImport> {
        if self.version.trim().is_empty() {
            return Err(PortError::Unsupported(
                "a captured producer version is required".into(),
            ));
        }
        let mut events = Vec::new();
        let mut usage = Vec::new();
        let mut unknown = BTreeMap::<String, u32>::new();
        let mut completed = HashSet::new();
        let mut requests = HashSet::new();
        let mut stream_id: Option<String> = None;
        let mut inconsistent = false;
        let mut metadata = SessionMetadata {
            agent_version: Some(self.version.clone()),
            ..Default::default()
        };
        jsonl::read_lines(path, |record, _| {
            let source = SourceRef::new(FileId(0), record.offset, record.len, record.line_no);
            let v = record.value.as_ref().unwrap_or(&Value::Null);
            let mut raw_type = string(v, "type").unwrap_or("unreadable").to_owned();
            let mut kind = EventKind::Unrecognised;
            let mut extra_kinds = Vec::new();
            if record.value.is_some() {
                let id = match self.surface {
                    StreamSurface::CodexExec => string(v, "thread_id"),
                    StreamSurface::CodexAppServer => v
                        .pointer("/params/threadId")
                        .and_then(Value::as_str)
                        .or_else(|| v.pointer("/params/thread/id").and_then(Value::as_str)),
                    StreamSurface::ClaudeStream
                        if v.get("parent_tool_use_id").is_some_and(|x| !x.is_null()) =>
                    {
                        None
                    }
                    StreamSurface::ClaudeStream => string(v, "session_id"),
                };
                if let Some(id) = id {
                    if stream_id.as_deref().is_some_and(|previous| previous != id) {
                        inconsistent = true;
                    }
                    stream_id.get_or_insert_with(|| id.to_owned());
                }
                match self.surface {
                    StreamSurface::CodexExec => match raw_type.as_str() {
                        "thread.started" => kind = EventKind::SessionStarted,
                        "turn.started" => kind = EventKind::TurnStarted,
                        "turn.completed" => {
                            kind = lifecycle("turn.completed");
                            if let Some(reported) = v.get("usage") {
                                usage.push(StreamUsage {
                                    source,
                                    scope: "turn-aggregate".into(),
                                    reported: reported.clone(),
                                });
                            }
                        }
                        "item.completed" => {
                            let item = &v["item"];
                            raw_type = format!(
                                "item.completed/{}",
                                string(item, "type").unwrap_or("unknown")
                            );
                            kind = finalized(item, false, &mut completed);
                        }
                        "item.started" | "item.updated" | "turn.failed" | "error" => {
                            kind = lifecycle(&raw_type)
                        }
                        _ => {}
                    },
                    StreamSurface::CodexAppServer => {
                        raw_type = string(v, "method").unwrap_or("rpc-response").to_owned();
                        let params = &v["params"];
                        match raw_type.as_str() {
                            "thread/started" => kind = EventKind::SessionStarted,
                            "turn/started" => kind = EventKind::TurnStarted,
                            "item/completed" => {
                                let item = &params["item"];
                                raw_type = format!(
                                    "item/completed/{}",
                                    string(item, "type").unwrap_or("unknown")
                                );
                                kind = finalized(item, true, &mut completed);
                            }
                            "thread/tokenUsage/updated" => {
                                kind = lifecycle(&raw_type);
                                if let Some(last) = params.pointer("/tokenUsage/last") {
                                    usage.push(StreamUsage {
                                        source,
                                        scope: "last-request-snapshot".into(),
                                        reported: last.clone(),
                                    });
                                }
                                metadata.context_window = params
                                    .pointer("/tokenUsage/modelContextWindow")
                                    .and_then(Value::as_u64)
                                    .and_then(|n| n.try_into().ok());
                            }
                            "turn/completed"
                            | "item/started"
                            | "item/agentMessage/delta"
                            | "item/reasoning/summaryTextDelta"
                            | "item/commandExecution/outputDelta" => kind = lifecycle(&raw_type),
                            "rpc-response"
                                if v.get("id").is_some()
                                    && (v.get("result").is_some() || v.get("error").is_some()) =>
                            {
                                kind = lifecycle(&raw_type)
                            }
                            _ => {}
                        }
                    }
                    StreamSurface::ClaudeStream => match raw_type.as_str() {
                        "system" => {
                            raw_type =
                                format!("system/{}", string(v, "subtype").unwrap_or("unknown"));
                            if raw_type == "system/init" {
                                kind = EventKind::SessionStarted;
                                if let Some(version) = string(v, "claude_code_version") {
                                    inconsistent |= version != self.version;
                                }
                                metadata.model = string(v, "model").map(str::to_owned);
                            }
                        }
                        "assistant" | "user" => {
                            // A subagent has its own context. Keep its envelope
                            // visible without folding its content/usage into the parent.
                            if v.get("parent_tool_use_id").is_some_and(|x| !x.is_null()) {
                                kind = lifecycle("subagent-message");
                            } else {
                                let message = &v["message"];
                                let mut kinds = claude_message(message, &raw_type);
                                kind = kinds.remove(0);
                                extra_kinds = kinds;
                                let key = string(v, "request_id").or_else(|| string(message, "id"));
                                if let (Some(key), Some(reported)) = (key, message.get("usage")) {
                                    if requests.insert(key.to_owned()) {
                                        usage.push(StreamUsage {
                                            source,
                                            scope: "request".into(),
                                            reported: reported.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        "result" => {
                            kind = lifecycle("result");
                            if let Some(reported) = v.get("usage") {
                                usage.push(StreamUsage {
                                    source,
                                    scope: "session-aggregate".into(),
                                    reported: reported.clone(),
                                });
                            }
                        }
                        "rate_limit_event" => kind = lifecycle(&raw_type),
                        // Deltas are presentation fragments, not additional
                        // finalized messages. Unknown nested events still warn.
                        "stream_event" => {
                            let nested = v
                                .pointer("/event/type")
                                .and_then(Value::as_str)
                                .unwrap_or("unknown");
                            raw_type = format!("stream_event/{nested}");
                            if matches!(
                                nested,
                                "message_start"
                                    | "message_delta"
                                    | "message_stop"
                                    | "content_block_start"
                                    | "content_block_delta"
                                    | "content_block_stop"
                                    | "ping"
                            ) {
                                kind = lifecycle(&raw_type);
                            }
                        }
                        _ => {}
                    },
                }
            }
            if matches!(kind, EventKind::Unrecognised) {
                *unknown.entry(raw_type.clone()).or_default() += 1;
            }
            events.push(Event {
                id: EventId::Ordinal(record.line_no),
                sequence: events.len().min(u32::MAX as usize) as u32,
                timestamp: None,
                kind,
                source,
                raw_type,
                turn: None,
                links: Default::default(),
                content_measurement: None,
            });
            for (index, kind) in extra_kinds.into_iter().enumerate() {
                let raw_type = format!("claude-message/block-{}", index + 1);
                if matches!(kind, EventKind::Unrecognised) {
                    *unknown.entry(raw_type.clone()).or_default() += 1;
                }
                events.push(Event {
                    id: EventId::Uuid(format!("stream:{}:{}", record.line_no, index + 1)),
                    sequence: events.len().min(u32::MAX as usize) as u32,
                    timestamp: None,
                    kind,
                    source,
                    raw_type,
                    turn: None,
                    links: Default::default(),
                    content_measurement: None,
                });
            }
        })?;
        if inconsistent {
            return Err(PortError::Malformed {
                path: path.display().to_string(),
                detail: "capture mixes sessions or disagrees with the declared producer version"
                    .into(),
            });
        }
        if stream_id.is_none() {
            return Err(PortError::Malformed {
                path: path.display().to_string(),
                detail: "no matching stream session identity; check --surface".into(),
            });
        }
        let id = SessionId::new(stream_id.unwrap())
            .map_err(|e| PortError::Unsupported(e.to_string()))?;
        Ok(StreamImport {surface:self.surface.label().into(),producer_version:self.version.clone(),
            session:AgentSession::new(id,self.surface.agent(),metadata,events,Vec::new(),unknown.into_iter().collect()),usage,
            limitations:vec!["This is a presentation timeline, not persisted request history; context reconstruction and exact recount are unavailable.".into(),"Usage scopes are retained separately; turn/session aggregates and repeated snapshots are not added or treated as prompt sizes.".into()]})
    }
}
fn string<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key)?.as_str()
}
fn lifecycle(s: &str) -> EventKind {
    EventKind::SessionEvent { subtype: s.into() }
}
fn message(role: MessageRole, text: String) -> EventKind {
    EventKind::Message {
        role,
        char_len: text.chars().count().min(u32::MAX as usize) as u32,
        preview: ct_domain::ports::truncate_chars(&text, 160),
    }
}
fn claude_message(v: &Value, role: &str) -> Vec<EventKind> {
    let blocks = v.get("content");
    if let Some(text) = blocks.and_then(Value::as_str) {
        return vec![message(
            if role == "user" {
                MessageRole::User
            } else {
                MessageRole::Assistant
            },
            text.into(),
        )];
    }
    let Some(blocks) = blocks.and_then(Value::as_array) else {
        return vec![EventKind::Unrecognised];
    };
    let mut kinds: Vec<_> = blocks
        .iter()
        .map(|block| match string(block, "type") {
            Some("text") => message(
                if role == "user" {
                    MessageRole::User
                } else {
                    MessageRole::Assistant
                },
                string(block, "text").unwrap_or_default().into(),
            ),
            Some("thinking") => EventKind::Reasoning {
                char_len: string(block, "thinking")
                    .unwrap_or_default()
                    .chars()
                    .count()
                    .min(u32::MAX as usize) as u32,
                redacted: string(block, "thinking").is_none_or(str::is_empty),
            },
            Some("redacted_thinking") => EventKind::Reasoning {
                char_len: 0,
                redacted: true,
            },
            Some("tool_use") => EventKind::ToolCall {
                tool: string(block, "name").unwrap_or("unknown").into(),
                call_id: string(block, "id").map(str::to_owned),
                char_len: block
                    .get("input")
                    .map(|v| v.to_string().chars().count().min(u32::MAX as usize) as u32)
                    .unwrap_or(0),
                target: block.get("input").and_then(crate::tool_target::describe),
            },
            Some("tool_result") => EventKind::ToolResult {
                tool: None,
                call_id: string(block, "tool_use_id").map(str::to_owned),
                char_len: block
                    .get("content")
                    .and_then(Value::as_str)
                    .map(|s| s.chars().count().min(u32::MAX as usize) as u32)
                    .unwrap_or(0),
                is_error: block
                    .get("is_error")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            },
            Some("image" | "document") => EventKind::ContextInjection {
                mechanism: "stream-media".into(),
                label: "Media/document block [tokens unmeasured]".into(),
                char_len: 0,
            },
            _ => EventKind::Unrecognised,
        })
        .collect();
    if kinds.is_empty() {
        kinds.push(EventKind::Unrecognised);
    }
    kinds
}
fn finalized(item: &Value, camel: bool, completed: &mut HashSet<String>) -> EventKind {
    let Some(id) = string(item, "id") else {
        return EventKind::Unrecognised;
    };
    if !completed.insert(id.into()) {
        return lifecycle("repeated-finalized-item");
    }
    match string(item, "type") {
        Some("agent_message" | "agentMessage") => message(
            MessageRole::Assistant,
            string(item, "text").unwrap_or_default().into(),
        ),
        Some("userMessage") => {
            let Some(blocks) = item["content"].as_array() else {
                return EventKind::Unrecognised;
            };
            if blocks.iter().any(|b| string(b, "type") != Some("text")) {
                return EventKind::Unrecognised;
            }
            message(
                MessageRole::User,
                blocks
                    .iter()
                    .filter_map(|b| string(b, "text"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        }
        Some("reasoning") => {
            let chars = if camel {
                item.get("summary")
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(|s| s.chars().count())
                            .sum::<usize>()
                    })
                    .unwrap_or(0)
            } else {
                string(item, "text").unwrap_or_default().chars().count()
            };
            EventKind::Reasoning {
                char_len: chars.min(u32::MAX as usize) as u32,
                redacted: false,
            }
        }
        Some("command_execution" | "commandExecution") => {
            let output = string(
                item,
                if camel {
                    "aggregatedOutput"
                } else {
                    "aggregated_output"
                },
            )
            .unwrap_or_default();
            EventKind::ToolResult {
                tool: Some("shell".into()),
                call_id: Some(id.into()),
                char_len: output.chars().count().min(u32::MAX as usize) as u32,
                is_error: item
                    .get(if camel { "exitCode" } else { "exit_code" })
                    .and_then(Value::as_i64)
                    .is_some_and(|x| x != 0),
            }
        }
        // These known presentation objects carry no literal request history.
        Some("todo_list" | "plan" | "error") => lifecycle("presentation-item"),
        _ => EventKind::Unrecognised,
    }
}
impl AgentAdapter for StreamAdapter {
    fn agent(&self) -> AgentKind {
        self.surface.agent()
    }
    fn roots(&self) -> Vec<String> {
        vec![self.root.display().to_string()]
    }
    fn discover(&self) -> PortResult<Vec<SessionDescriptor>> {
        find_files(&self.root, |p| has_extension(p, "jsonl"))
            .into_iter()
            .map(|path| {
                let report = self.import(&path)?;
                Ok(SessionDescriptor {
                    id: report.session.id().clone(),
                    agent: self.agent(),
                    path: path.display().to_string(),
                    size_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                    project: None,
                    title: None,
                    git_branch: None,
                    started_at: None,
                    last_activity: None,
                    thread_role: ThreadRole::Root,
                })
            })
            .collect()
    }
    fn load(&self, d: &SessionDescriptor) -> PortResult<AgentSession> {
        self.import(Path::new(&d.path)).map(|r| r.session)
    }
    fn reconstruct(
        &self,
        _: &AgentSession,
        _: TurnNumber,
        _: &dyn TokenEstimator,
    ) -> PortResult<ReconstructedContext> {
        Err(PortError::Unsupported(
            "stdout/app-server captures do not contain complete persisted request histories".into(),
        ))
    }
    fn recount_exact(
        &self,
        _: &AgentSession,
        _: &mut [ct_domain::ContextItem],
        _: &dyn ct_domain::ports::RawEventSource,
        _: &dyn TokenEstimator,
    ) -> PortResult<ct_domain::ports::ExactRecount> {
        Err(PortError::Unsupported("presentation streams do not expose complete model-visible request items for an exact recount".into()))
    }
}
