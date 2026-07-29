//! Exact structural diffing of Codex `replacement_history` arrays.
//!
//! This is deliberately separate from reconstruction: reconstruction needs one
//! compact summary item and never re-reads raw data; this explicit report pays
//! the raw-read cost to compare the literal API items without exposing them.

use super::parse;
use crate::jsonl::MAX_PARSE_BYTES;
use ct_domain::model::event::EventKind;
use ct_domain::ports::{RawEventSource, TokenEstimator};
use ct_domain::{
    AgentSession, CompactionDiff, CompactionDiffItem, CompactionDiffUnavailable,
    CompactionItemDisposition, Confidence, MessageRole, Provenance, SourceRef, TokenCount,
};
use serde_json::Value;

#[derive(Clone)]
struct LiveItem {
    value: Value,
    source: SourceRef,
}

/// Produce one result per compaction. A bad raw line affects only the
/// compaction that needs it; a later valid replacement re-establishes a known
/// history for the next diff.
pub fn diff(
    session: &AgentSession,
    raw: &dyn RawEventSource,
    estimator: &dyn TokenEstimator,
) -> Vec<CompactionDiff> {
    let mut live = Vec::<LiveItem>::new();
    let mut history_known = true;
    let mut out = Vec::new();

    for event in session.events() {
        if let EventKind::Compacted(_) = event.kind {
            let replacement = replacement_history(event.source, raw);
            let result = match replacement {
                Err(reason) => {
                    live.clear();
                    history_known = false;
                    CompactionDiff::Unavailable {
                        source: event.source,
                        turn: event.turn,
                        reason,
                    }
                }
                Ok(replacement) => {
                    let result = if !history_known {
                        CompactionDiff::Unavailable {
                            source: event.source,
                            turn: event.turn,
                            reason: CompactionDiffUnavailable::UnknownPrecedingHistory,
                        }
                    } else {
                        match resolve_live(&live, raw) {
                            Err(reason) => CompactionDiff::Unavailable {
                                source: event.source,
                                turn: event.turn,
                                reason,
                            },
                            Ok(before) => CompactionDiff::Available {
                                source: event.source,
                                turn: event.turn,
                                items: compare(before, &replacement, event.source, estimator),
                            },
                        }
                    };
                    live = replacement
                        .into_iter()
                        .map(|value| LiveItem {
                            value,
                            source: event.source,
                        })
                        .collect();
                    history_known = true;
                    result
                }
            };
            out.push(result);
            continue;
        }

        // Codex defines every `response_item` as a literal API history item.
        // Use the envelope type, not our current event classifier, so an
        // upstream item type we do not yet understand cannot be silently
        // dropped from an "exact" report.
        if event.raw_type == "response_item" || event.raw_type.starts_with("response_item/") {
            live.push(LiveItem {
                value: Value::Null,
                source: event.source,
            });
        }
    }
    out
}

fn replacement_history(
    source: SourceRef,
    raw: &dyn RawEventSource,
) -> Result<Vec<Value>, CompactionDiffUnavailable> {
    if source.byte_len as usize > MAX_PARSE_BYTES {
        return Err(CompactionDiffUnavailable::OversizedRawLine);
    }
    let line = raw
        .fetch(source)
        .map_err(|_| CompactionDiffUnavailable::UnavailableRawLine)?;
    let root: Value = serde_json::from_str(line.trim())
        .map_err(|_| CompactionDiffUnavailable::MalformedRawLine)?;
    let history = root
        .get("payload")
        .and_then(|p| p.get("replacement_history"))
        .and_then(Value::as_array)
        .ok_or(CompactionDiffUnavailable::MissingReplacementHistory)?;
    Ok(history.clone())
}

fn resolve_live(
    live: &[LiveItem],
    raw: &dyn RawEventSource,
) -> Result<Vec<LiveItem>, CompactionDiffUnavailable> {
    live.iter()
        .map(|item| {
            if !item.value.is_null() {
                return Ok(item.clone());
            }
            if item.source.byte_len as usize > MAX_PARSE_BYTES {
                return Err(CompactionDiffUnavailable::OversizedRawLine);
            }
            let line = raw
                .fetch(item.source)
                .map_err(|_| CompactionDiffUnavailable::UnavailableRawLine)?;
            let root: Value = serde_json::from_str(line.trim())
                .map_err(|_| CompactionDiffUnavailable::MalformedPrecedingItem)?;
            let payload = root
                .get("payload")
                .filter(|_| root.get("type").and_then(Value::as_str) == Some("response_item"))
                .cloned()
                .ok_or(CompactionDiffUnavailable::MalformedPrecedingItem)?;
            Ok(LiveItem {
                value: payload,
                source: item.source,
            })
        })
        .collect()
}

fn compare(
    before: Vec<LiveItem>,
    replacement: &[Value],
    replacement_source: SourceRef,
    estimator: &dyn TokenEstimator,
) -> Vec<CompactionDiffItem> {
    let mut claimed = vec![false; replacement.len()];
    let mut items = Vec::new();

    for (ordinal, item) in before.iter().enumerate() {
        let matched = replacement
            .iter()
            .enumerate()
            .find(|(index, value)| !claimed[*index] && *value == &item.value)
            .map(|(index, _)| index);
        if let Some(index) = matched {
            claimed[index] = true;
        }
        items.push(describe(
            ordinal as u32,
            &item.value,
            if matched.is_some() {
                CompactionItemDisposition::Preserved
            } else {
                CompactionItemDisposition::Dropped
            },
            item.source,
            estimator,
        ));
    }

    for (ordinal, value) in replacement.iter().enumerate() {
        if !claimed[ordinal] {
            items.push(describe(
                ordinal as u32,
                value,
                CompactionItemDisposition::AddedByReplacement,
                replacement_source,
                estimator,
            ));
        }
    }
    items
}

fn describe(
    ordinal: u32,
    value: &Value,
    disposition: CompactionItemDisposition,
    source: SourceRef,
    estimator: &dyn TokenEstimator,
) -> CompactionDiffItem {
    let item_type = value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let role = value
        .get("role")
        .and_then(Value::as_str)
        .and_then(message_role);
    let text_tokens = parse::content_text(value)
        .map(|text| estimator.count_text(&text))
        .filter(TokenCount::is_trustworthy);
    CompactionDiffItem {
        ordinal,
        item_type,
        role,
        disposition,
        normalized_json_bytes: serde_json::to_vec(value)
            .map(|bytes| bytes.len().min(u32::MAX as usize) as u32)
            .unwrap_or(0),
        text_tokens,
        provenance: Provenance {
            confidence: Confidence::Derived,
            source: Some(source),
        },
    }
}

fn message_role(role: &str) -> Option<MessageRole> {
    Some(match role {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "developer" => MessageRole::Developer,
        "system" => MessageRole::System,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizers::TiktokenEstimator;
    use ct_domain::model::event::{CompactionFacts, EventLinks};
    use ct_domain::ports::{PortError, PortResult};
    use ct_domain::{AgentKind, Event, EventId, FileId, SessionId, SessionMetadata, Turn};

    struct Lines(Vec<Result<&'static str, PortError>>);
    impl RawEventSource for Lines {
        fn fetch(&self, source: SourceRef) -> PortResult<String> {
            self.0[source.line_no as usize - 1]
                .as_ref()
                .map(|s| (*s).to_string())
                .map_err(|e| PortError::Io(e.to_string()))
        }
    }

    fn event(line: u32, kind: EventKind, raw_type: &str) -> Event {
        Event {
            id: EventId::Ordinal(line),
            sequence: line - 1,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), 0, 500, line),
            raw_type: raw_type.into(),
            turn: None,
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    fn session(events: Vec<Event>) -> AgentSession {
        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::Codex,
            SessionMetadata::default(),
            events,
            Vec::<Turn>::new(),
            vec![],
        )
    }

    fn estimator() -> TiktokenEstimator {
        TiktokenEstimator::o200k().unwrap()
    }

    #[test]
    fn reports_dropped_preserved_and_opaque_replacement_without_content() {
        let events = vec![
            event(1, EventKind::Unrecognised, "response_item/message"),
            event(
                2,
                EventKind::ToolResult {
                    tool: None,
                    call_id: None,
                    char_len: 1,
                    is_error: false,
                },
                "response_item/function_call_output",
            ),
            event(
                3,
                EventKind::Compacted(CompactionFacts {
                    replacement_recorded: true,
                    ..Default::default()
                }),
                "compacted",
            ),
        ];
        let lines = Lines(vec![
            Ok(
                r#"{"type":"response_item","payload":{"type":"message","role":"user","content":[{"type":"input_text","text":"keep"}]}}"#,
            ),
            Ok(
                r#"{"type":"response_item","payload":{"type":"function_call_output","call_id":"c","output":"drop"}}"#,
            ),
            Ok(
                r#"{"type":"compacted","payload":{"replacement_history":[{"type":"message","role":"user","content":[{"type":"input_text","text":"keep"}]},{"type":"compaction","encrypted_content":"opaque"}]}}"#,
            ),
        ]);
        let report = diff(&session(events), &lines, &estimator());
        let CompactionDiff::Available { items, .. } = &report[0] else {
            panic!("expected diff")
        };
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].disposition, CompactionItemDisposition::Preserved);
        assert!(items[0].text_tokens.is_some());
        assert_eq!(items[1].disposition, CompactionItemDisposition::Dropped);
        assert_eq!(items[1].item_type, "function_call_output");
        assert_eq!(
            items[2].disposition,
            CompactionItemDisposition::AddedByReplacement
        );
        assert_eq!(items[2].item_type, "compaction");
        assert!(items[2].text_tokens.is_none());
    }

    #[test]
    fn malformed_or_unavailable_raw_is_named_not_guessed() {
        let events = vec![event(
            1,
            EventKind::Compacted(CompactionFacts::default()),
            "compacted",
        )];
        let malformed = Lines(vec![Ok("not json")]);
        assert!(matches!(
            diff(&session(events.clone()), &malformed, &estimator())[0],
            CompactionDiff::Unavailable {
                reason: CompactionDiffUnavailable::MalformedRawLine,
                ..
            }
        ));
        let unavailable = Lines(vec![Err(PortError::Io("gone".into()))]);
        assert!(matches!(
            diff(&session(events), &unavailable, &estimator())[0],
            CompactionDiff::Unavailable {
                reason: CompactionDiffUnavailable::UnavailableRawLine,
                ..
            }
        ));

        let missing = Lines(vec![Ok(r#"{"type":"compacted","payload":{}}"#)]);
        assert!(matches!(
            diff(
                &session(vec![event(
                    1,
                    EventKind::Compacted(CompactionFacts::default()),
                    "compacted"
                )]),
                &missing,
                &estimator()
            )[0],
            CompactionDiff::Unavailable {
                reason: CompactionDiffUnavailable::MissingReplacementHistory,
                ..
            }
        ));
    }

    #[test]
    fn a_valid_replacement_reestablishes_history_after_a_bad_one() {
        let events = vec![
            event(
                1,
                EventKind::Compacted(CompactionFacts::default()),
                "compacted",
            ),
            event(
                2,
                EventKind::Compacted(CompactionFacts::default()),
                "compacted",
            ),
            event(
                3,
                EventKind::Compacted(CompactionFacts::default()),
                "compacted",
            ),
        ];
        let lines = Lines(vec![
            Ok("not json"),
            Ok(r#"{"type":"compacted","payload":{"replacement_history":[]}}"#),
            Ok(r#"{"type":"compacted","payload":{"replacement_history":[]}}"#),
        ]);
        let report = diff(&session(events), &lines, &estimator());
        assert!(matches!(report[0], CompactionDiff::Unavailable { .. }));
        assert!(matches!(
            report[1],
            CompactionDiff::Unavailable {
                reason: CompactionDiffUnavailable::UnknownPrecedingHistory,
                ..
            }
        ));
        assert!(matches!(report[2], CompactionDiff::Available { .. }));
    }

    #[test]
    fn an_oversized_compaction_is_not_fetched_or_parsed() {
        let mut event = event(
            1,
            EventKind::Compacted(CompactionFacts::default()),
            "compacted",
        );
        event.source.byte_len = (MAX_PARSE_BYTES + 1) as u32;
        let report = diff(&session(vec![event]), &Lines(vec![]), &estimator());
        assert!(matches!(
            report[0],
            CompactionDiff::Unavailable {
                reason: CompactionDiffUnavailable::OversizedRawLine,
                ..
            }
        ));
    }
}
