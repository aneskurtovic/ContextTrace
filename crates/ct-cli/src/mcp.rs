//! Minimal stdio MCP server for ContextTrace's read-only queries.
//!
//! This intentionally implements the small JSON-RPC surface needed by MCP
//! clients instead of adding a transport dependency. Stdio is the transport,
//! and every operation below stays inside the existing local application
//! service and archive store.

use crate::{parse_sides, pick_turn, session_estimator};
use ct_adapters::FileRawEventSource;
use ct_application::{ContextTrace, SessionFilter, SessionSource};
use ct_domain::ports::{ArchiveStore, RawEventSource, RecordTransform};
use ct_domain::{AgentKind, ItemFilter};
use serde_json::{json, Map, Value};
use std::io::{self, BufRead, Write};

const PROTOCOL_VERSION: &str = "2024-11-05";

/// Serve newline-delimited JSON-RPC requests until stdin closes.
pub fn serve(
    app: &ContextTrace,
    archive: &dyn ArchiveStore,
) -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(error) => {
                write_response(
                    &mut stdout,
                    json!({
                        "jsonrpc": "2.0",
                        "id": Value::Null,
                        "error": {"code": -32700, "message": error.to_string()}
                    }),
                )?;
                continue;
            }
        };

        let Some(method) = request.get("method").and_then(Value::as_str) else {
            write_response(
                &mut stdout,
                error_response(&request, -32600, "missing method"),
            )?;
            continue;
        };
        // MCP notifications have no id and must not receive a response.
        if request.get("id").is_none() {
            continue;
        }

        let response = match method {
            "initialize" => success_response(&request, initialize_result()),
            "notifications/initialized" | "ping" => success_response(&request, json!({})),
            "tools/list" => success_response(&request, json!({"tools": tool_definitions()})),
            "tools/call" => match call_tool(
                request
                    .get("params")
                    .and_then(|p| p.get("name"))
                    .and_then(Value::as_str),
                request.get("params").and_then(|p| p.get("arguments")),
                app,
                archive,
            ) {
                Ok(value) => success_response(&request, tool_result(value)),
                Err(error) => success_response(&request, tool_error(error.to_string())),
            },
            _ => error_response(&request, -32601, "method not found"),
        };
        write_response(&mut stdout, response)?;
    }
    Ok(())
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": {"tools": {}},
        "serverInfo": {"name": "contexttrace", "version": env!("CARGO_PKG_VERSION")},
        "instructions": "All results are local. Confidence is preserved. Content recovery is redacted unless raw=true is explicitly requested."
    })
}

fn tool_definitions() -> Vec<Value> {
    let object = |properties: Value, required: &[&str]| {
        json!({
            "type": "object",
            "properties": properties,
            "required": required,
            "additionalProperties": false
        })
    };
    vec![
        json!({"name":"sessions","description":"List live sessions and archived sessions whose logs are gone.","inputSchema":object(json!({"agent":{"type":"string"},"project":{"type":"string"},"limit":{"type":"integer","minimum":1}}), &[])}),
        json!({"name":"inspect","description":"Inspect the normalized event timeline of one session. Content bytes are not returned.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"context","description":"Reconstruct the context composition at one turn, preserving confidence on every figure.","inputSchema":object(json!({"id":{"type":"string"},"turn":{"type":"integer","minimum":1}}), &["id"])}),
        json!({"name":"largest","description":"Rank the largest context contributors at one turn.","inputSchema":object(json!({"id":{"type":"string"},"turn":{"type":"integer","minimum":1},"limit":{"type":"integer","minimum":1}}), &["id"])}),
        json!({"name":"compactions","description":"Show recorded compaction facts and exact Codex replacement diffs where available.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"growth","description":"Return the session's prompt-growth timeline.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"residual","description":"Return the per-turn unlogged-context series when the session supports calibration.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"doctor","description":"Diagnose format fidelity and context spikes for one session.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"fidelity","description":"Show parse fidelity per turn and unassigned events.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"instructions","description":"Show observed instruction signatures and changes without returning instruction bodies.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"diff","description":"Compare two turns or sessions.","inputSchema":object(json!({"left":{"type":"string"},"right":{"type":"string"}}), &["left"])}),
        json!({"name":"secrets","description":"Find credential-shaped values without returning their values.","inputSchema":object(json!({"id":{"type":"string"}}), &["id"])}),
        json!({"name":"recover_context_item","description":"Recover one context item's raw record. Redacted by default; raw=true is an explicit opt-out and is recorded in the response.","inputSchema":object(json!({"id":{"type":"string"},"turn":{"type":"integer","minimum":1},"item":{"type":"string"},"raw":{"type":"boolean"}}), &["id","item"])}),
        json!({"name":"roots","description":"Show the local roots ContextTrace reads and the archive root it may write.","inputSchema":object(json!({}), &[])}),
    ]
}

fn call_tool(
    name: Option<&str>,
    arguments: Option<&Value>,
    app: &ContextTrace,
    archive: &dyn ArchiveStore,
) -> Result<Value, Box<dyn std::error::Error>> {
    let name = name.ok_or("tools/call requires a name")?;
    let args = arguments
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match name {
        "sessions" => {
            let filter = SessionFilter {
                agent: args
                    .get("agent")
                    .and_then(Value::as_str)
                    .map(|agent| {
                        AgentKind::parse(agent).ok_or_else(|| format!("unknown agent '{agent}'"))
                    })
                    .transpose()?,
                project: args
                    .get("project")
                    .and_then(Value::as_str)
                    .map(String::from),
                since: None,
                limit: Some(args.get("limit").and_then(Value::as_u64).unwrap_or(40) as usize),
            };
            Ok(with_source(
                serde_json::to_value(app.list_sessions_with_archive(&filter, archive)?)?,
                None,
                false,
            ))
        }
        "inspect" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            Ok(with_source(
                serde_json::to_value(session)?,
                Some(&resolved.source),
                false,
            ))
        }
        "context" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_content_analysis_and_archive(id, archive)?;
            let turn = pick_turn(
                app,
                &session,
                args.get("turn").and_then(Value::as_u64).map(|n| n as u32),
            )?;
            let calibrated = session_estimator(app, &session, resolved.binding);
            let (snapshot, recount) =
                calibrated.snapshot_for(app, &session, &resolved, turn, false)?;
            let report = snapshot
                .filtered(&ItemFilter::default())
                .composition_report();
            let recount = recount.map(|value| {
                json!({
                    "counted": value.counted,
                    "opaque": value.opaque,
                    "unavailable": value.unavailable,
                    "total": value.total()
                })
            });
            Ok(with_source(
                json!({"report": report, "recount": recount, "confidence": "preserved"}),
                Some(&resolved.source),
                false,
            ))
        }
        "largest" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            let turn = pick_turn(
                app,
                &session,
                args.get("turn").and_then(Value::as_u64).map(|n| n as u32),
            )?;
            let calibrated = session_estimator(app, &session, resolved.binding);
            let snapshot = calibrated.snapshot(app, &session, resolved.binding, turn)?;
            let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(15) as usize;
            Ok(with_source(
                serde_json::to_value(
                    snapshot
                        .filtered(&ItemFilter::default())
                        .contributor_report(limit),
                )?,
                Some(&resolved.source),
                false,
            ))
        }
        "compactions" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            let raw = FileRawEventSource::for_session(&resolved.descriptor.path);
            let report = app.compaction_diffs(&session, resolved.binding, &raw)?;
            Ok(with_source(
                serde_json::to_value(report)?,
                Some(&resolved.source),
                false,
            ))
        }
        "growth" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            Ok(with_source(
                serde_json::to_value(ct_application::timeline(&session))?,
                Some(&resolved.source),
                false,
            ))
        }
        "residual" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            let ratio = session_estimator(app, &session, resolved.binding)
                .ratio
                .ok_or("cannot measure unlogged context for this session")?;
            Ok(with_source(
                serde_json::to_value(app.residual_series(&session, resolved.binding, ratio))?,
                Some(&resolved.source),
                false,
            ))
        }
        "doctor" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            Ok(with_source(
                serde_json::to_value(app.diagnose(&session))?,
                Some(&resolved.source),
                false,
            ))
        }
        "fidelity" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            Ok(with_source(
                serde_json::to_value(ct_application::fidelity_trend(&session))?,
                Some(&resolved.source),
                false,
            ))
        }
        "instructions" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            Ok(with_source(
                serde_json::to_value(ct_application::instruction_drift(&session))?,
                Some(&resolved.source),
                false,
            ))
        }
        "diff" => {
            let left = required_string(&args, "left")?;
            let (left_spec, right_spec) =
                parse_sides(left, args.get("right").and_then(Value::as_str))?;
            let (left_session, left_resolved) = app.load_with_archive(&left_spec.id, archive)?;
            let right_loaded = (right_spec.id != left_spec.id)
                .then(|| app.load_with_archive(&right_spec.id, archive))
                .transpose()?;
            let (right_session, right_resolved) = match &right_loaded {
                Some((session, resolved)) => (session, resolved),
                None => (&left_session, &left_resolved),
            };
            let left_turn = pick_turn(app, &left_session, left_spec.turn)?;
            let right_turn = pick_turn(app, right_session, right_spec.turn)?;
            let left_cal = session_estimator(app, &left_session, left_resolved.binding);
            let right_cal_owned = right_loaded
                .as_ref()
                .map(|(session, resolved)| session_estimator(app, session, resolved.binding));
            let right_cal = right_cal_owned.as_ref().unwrap_or(&left_cal);
            let left_snapshot =
                left_cal.snapshot(app, &left_session, left_resolved.binding, left_turn)?;
            let right_snapshot =
                right_cal.snapshot(app, right_session, right_resolved.binding, right_turn)?;
            let diff = ct_application::compare(
                ct_application::Side {
                    snapshot: &left_snapshot,
                    instrument: left_cal.instrument(app, left_resolved.binding),
                },
                ct_application::Side {
                    snapshot: &right_snapshot,
                    instrument: right_cal.instrument(app, right_resolved.binding),
                },
            );
            Ok(with_source(
                serde_json::to_value(diff)?,
                Some(&left_resolved.source),
                false,
            ))
        }
        "secrets" => {
            let id = required_string(&args, "id")?;
            let (session, resolved) = app.load_with_archive(id, archive)?;
            let raw = FileRawEventSource::for_session(&resolved.descriptor.path);
            let report = app.scan_secrets(&session, &raw);
            let findings: Vec<Value> = report
                .findings
                .iter()
                .map(|finding| json!({"kind": finding.kind.label(), "occurrences": finding.occurrences, "turn": finding.turn, "line": finding.line_no, "event_type": finding.event_type}))
                .collect();
            Ok(with_source(
                json!({"findings": findings, "scanned_records": report.scanned_records, "unreadable_records": report.unreadable_records}),
                Some(&resolved.source),
                false,
            ))
        }
        "recover_context_item" => {
            let id = required_string(&args, "id")?;
            let needle = required_string(&args, "item")?;
            let raw_requested = args.get("raw").and_then(Value::as_bool).unwrap_or(false);
            let (session, resolved) = app.load_with_archive(id, archive)?;
            let turn = pick_turn(
                app,
                &session,
                args.get("turn").and_then(Value::as_u64).map(|n| n as u32),
            )?;
            let calibrated = session_estimator(app, &session, resolved.binding);
            let snapshot = calibrated.snapshot(app, &session, resolved.binding, turn)?;
            let item = snapshot
                .items()
                .iter()
                .find(|item| item.id.as_str() == needle)
                .ok_or_else(|| format!("no context item '{needle}' at turn {}", turn.get()))?;
            let source = item
                .provenance
                .source
                .ok_or("this context item has no recoverable source record")?;
            let text = FileRawEventSource::for_session(&resolved.descriptor.path).fetch(source)?;
            let content = if raw_requested {
                text
            } else {
                RedactionTransform::apply_record(&text)
            };
            Ok(with_source(
                json!({"item": item, "source": source, "content": content, "confidence": item.confidence(), "raw_requested": raw_requested}),
                Some(&resolved.source),
                raw_requested,
            ))
        }
        "roots" => Ok(json!({
            "read": app.roots().into_iter().map(|(agent, paths)| json!({"agent": agent, "paths": paths})).collect::<Vec<_>>(),
            "archive": archive.root()
        })),
        _ => Err(format!("unknown tool '{name}'").into()),
    }
}

struct RedactionTransform;

impl RedactionTransform {
    fn apply_record(record: &str) -> String {
        let transform = ct_application::RedactingTransform;
        transform.apply(record).0.into_owned()
    }
}

fn required_string<'a>(
    args: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str, Box<dyn std::error::Error>> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("missing string argument '{key}'").into())
}

fn with_source(value: Value, source: Option<&SessionSource>, raw_requested: bool) -> Value {
    let value = if raw_requested {
        value
    } else {
        redact_value(value)
    };
    json!({
        "data": value,
        "evidence": source.map(source_metadata).unwrap_or_else(|| json!({"kind":"local"})),
        "redaction": if raw_requested { "raw" } else { "redacted-by-default" },
        "confidence": "preserved"
    })
}

/// Apply the archive/export redactor to every JSON string value returned over
/// MCP. This keeps previews, labels and diagnostic candidates subject to the
/// same default as recovered record content without inventing a second secret
/// vocabulary in the transport layer.
fn redact_value(value: Value) -> Value {
    let encoded = match serde_json::to_string(&value) {
        Ok(encoded) => encoded,
        Err(_) => return value,
    };
    let redacted = RedactionTransform::apply_record(&encoded);
    serde_json::from_str(&redacted).unwrap_or(value)
}

fn source_metadata(source: &SessionSource) -> Value {
    match source {
        SessionSource::Live => json!({"kind": "live-log"}),
        SessionSource::Archive(entry) => json!({
            "kind": "archive",
            "archived_at": entry.archived_at,
            "redaction": entry.redaction,
            "differs_from_source": entry.differs_from_source(),
            "source_path": entry.descriptor.path,
            "source_digest": entry.source_digest,
            "archived_digest": entry.archived_digest
        }),
    }
}

fn tool_result(value: Value) -> Value {
    json!({
        "content": [{"type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".into())}],
        "structuredContent": value,
        "isError": false
    })
}

fn tool_error(message: String) -> Value {
    json!({
        "content": [{"type": "text", "text": message}],
        "isError": true
    })
}

fn success_response(request: &Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": request.get("id").cloned().unwrap_or(Value::Null), "result": result})
}

fn error_response(request: &Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": request.get("id").cloned().unwrap_or(Value::Null), "error": {"code": code, "message": message}})
}

fn write_response(writer: &mut impl Write, response: Value) -> io::Result<()> {
    serde_json::to_writer(&mut *writer, &response)?;
    writer.write_all(b"\n")?;
    writer.flush()
}
