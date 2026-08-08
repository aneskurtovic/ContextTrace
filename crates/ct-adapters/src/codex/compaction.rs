//! Exact structural diffing of Codex `replacement_history` arrays.
//!
//! This is deliberately separate from reconstruction: reconstruction needs one
//! compact summary item and never re-reads raw data; this explicit report pays
//! the raw-read cost to compare the literal API items without exposing them.

use super::parse;
use crate::fingerprint;
use crate::jsonl::MAX_PARSE_BYTES;
use ct_domain::model::event::EventKind;
use ct_domain::ports::{RawEventSource, TokenEstimator};
use ct_domain::{
    AgentSession, CompactionDiff, CompactionDiffItem, CompactionDiffUnavailable,
    CompactionItemDisposition, Confidence, ContentFingerprint, MessageRole, Provenance, SourceRef,
    TokenCount,
};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

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
    // One fingerprint bucket per distinct value, holding candidate
    // replacement indices in ascending order. Matching a pre-compaction item
    // is then a pop from the front of its bucket instead of a scan of the
    // whole replacement array, which is what makes this linear rather than
    // quadratic. Duplicates are exactly why a plain `HashMap<Fingerprint,
    // usize>` would be wrong: the previous quadratic scan paired the Nth
    // occurrence of a value in `before` with the Nth unclaimed occurrence in
    // `replacement`, in index order, and a single-slot map would collapse
    // that into "first one wins, the rest all drop." The queue reproduces
    // the original pairing exactly.
    let mut by_fingerprint: HashMap<ContentFingerprint, VecDeque<u32>> = HashMap::new();
    for (replacement_index, value) in replacement.iter().enumerate() {
        by_fingerprint
            .entry(identity_fingerprint(value))
            .or_default()
            .push_back(replacement_index as u32);
    }

    let mut claimed = vec![false; replacement.len()];
    let mut items = Vec::with_capacity(before.len() + replacement.len());

    for (history_index, item) in before.iter().enumerate() {
        let fingerprint = identity_fingerprint(&item.value);
        let matched = by_fingerprint
            .get_mut(&fingerprint)
            .and_then(VecDeque::pop_front);
        let disposition = match matched {
            Some(replacement_index) => {
                claimed[replacement_index as usize] = true;
                CompactionItemDisposition::Preserved {
                    history_index: history_index as u32,
                    replacement_index,
                }
            }
            None => CompactionItemDisposition::Dropped {
                history_index: history_index as u32,
            },
        };
        items.push(describe(&item.value, disposition, item.source, estimator));
    }

    for (replacement_index, value) in replacement.iter().enumerate() {
        if !claimed[replacement_index] {
            items.push(describe(
                value,
                CompactionItemDisposition::AddedByReplacement {
                    replacement_index: replacement_index as u32,
                },
                replacement_source,
                estimator,
            ));
        }
    }
    items
}

/// A fingerprint whose equality matches `serde_json::Value`'s `PartialEq`
/// exactly, so substituting it for `item.value == value` cannot change which
/// items are dropped, preserved or added.
///
/// This deliberately does not call `fingerprint::value` from this crate's
/// `fingerprint` module. That function is the right tool for near-duplicate
/// *content* detection (its own doc comment says so) and, in service of
/// that, hashes objects in raw parse order and special-cases bare JSON
/// strings so they match the same text wrapped in an object. Both choices
/// are wrong for *exact* identity matching here:
///
/// - `serde_json::Value`'s object equality is order-independent — it
///   delegates to `serde_json::Map`, which (with `preserve_order` on, as
///   this workspace has it) is backed by `indexmap::IndexMap`, whose
///   `PartialEq` compares key/value pairs regardless of position. Hashing
///   parse order, as `fingerprint::value` does, is therefore *stricter*
///   than the equality it would stand in for: two values `compare` treats
///   as identical today could get different fingerprints if their key order
///   ever differs, silently turning a `Preserved` into a `Dropped` +
///   `AddedByReplacement` pair.
/// - `fingerprint::value` hashes a bare `Value::String` as its raw text
///   rather than its quoted JSON form, so `Value::String("null")` and
///   `Value::Null` — which are *not* equal under `Value::eq` — would
///   fingerprint identically. That is a real collision, not a hypothetical
///   one, and exact matching cannot tolerate it.
///
/// This function fixes both: it recursively sorts object keys before
/// serializing (matching `IndexMap`'s order-independent equality) while
/// leaving arrays positional (matching `Vec`'s equality) and always
/// serializing through `Value::to_string()`, which keeps every variant's
/// punctuation (quotes, brackets) intact so no two differently-typed values
/// can collide. The one accepted gap: IEEE 754 says `-0.0 == 0.0`, so two
/// `Number`s that `Value::eq` treats as equal could serialize to `"-0.0"`
/// and `"0.0"` and fingerprint differently. Valid Codex Responses API JSON
/// has no reason to emit a negative zero, and JSON has no `NaN`/`Infinity`
/// to worry about, so this is accepted rather than defended against.
fn identity_fingerprint(value: &Value) -> ContentFingerprint {
    fingerprint::text(&canonical_json(value).to_string()).fingerprint
}

/// Recursively sort object keys; leave every other `Value` variant as-is.
fn canonical_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let mut sorted = serde_json::Map::with_capacity(map.len());
            for key in keys {
                sorted.insert(key.clone(), canonical_json(&map[key]));
            }
            Value::Object(sorted)
        }
        Value::Array(items) => Value::Array(items.iter().map(canonical_json).collect()),
        other => other.clone(),
    }
}

fn describe(
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
    use serde_json::json;

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

    // CT-066: the quadratic scan paired the Nth occurrence of a value in
    // `before` with the Nth *unclaimed* occurrence in `replacement`, walking
    // both lists in ascending index order. A naive `HashMap<Fingerprint,
    // usize>` rewrite would instead let every occurrence after the first
    // collide on one slot, so this fixes three duplicate `dup` items in
    // `before` against two in `replacement` and checks the exact pairing,
    // including which occurrence gets `Dropped` when a side runs out, and
    // that `replacement_index` (not just `history_index`) is preserved
    // for each match. This test was run against the pre-CT-066 quadratic
    // `compare` (by temporarily reverting just the matching loop) before the
    // fingerprint rewrite landed, and passed unchanged.
    #[test]
    fn duplicate_values_pair_off_in_ascending_index_order_on_both_sides() {
        let source = SourceRef::new(FileId(0), 0, 500, 1);
        let dup = json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "dup"}],
        });
        let unique_before = json!({
            "type": "message",
            "role": "user",
            "content": [{"type": "input_text", "text": "only in before"}],
        });
        let other_added = json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "only in replacement A"}],
        });
        let unique_after = json!({
            "type": "message",
            "role": "assistant",
            "content": [{"type": "output_text", "text": "only in replacement B"}],
        });

        // 3 copies of `dup` in `before`, only 2 in `replacement`.
        let before = vec![
            LiveItem {
                value: dup.clone(),
                source,
            },
            LiveItem {
                value: dup.clone(),
                source,
            },
            LiveItem {
                value: dup.clone(),
                source,
            },
            LiveItem {
                value: unique_before.clone(),
                source,
            },
        ];
        let replacement = vec![
            other_added.clone(),
            dup.clone(),
            dup.clone(),
            unique_after.clone(),
        ];

        let items = compare(before, &replacement, source, &estimator());
        let dispositions: Vec<CompactionItemDisposition> =
            items.iter().map(|item| item.disposition).collect();

        assert_eq!(
            dispositions,
            vec![
                // before[0] claims the first unclaimed `dup`, at replacement[1].
                CompactionItemDisposition::Preserved {
                    history_index: 0,
                    replacement_index: 1,
                },
                // before[1] claims the next one, at replacement[2].
                CompactionItemDisposition::Preserved {
                    history_index: 1,
                    replacement_index: 2,
                },
                // before[2] is the third `dup`; both replacement copies are
                // already claimed, so it drops rather than reusing one.
                CompactionItemDisposition::Dropped { history_index: 2 },
                CompactionItemDisposition::Dropped { history_index: 3 },
                // Both unclaimed replacement items are reported in ascending
                // replacement_index order.
                CompactionItemDisposition::AddedByReplacement {
                    replacement_index: 0
                },
                CompactionItemDisposition::AddedByReplacement {
                    replacement_index: 3
                },
            ]
        );
    }

    // CT-066: demonstrates the linear-scan claim rather than just asserting
    // it. Not run by default (timing assertions in a normal CI pass are
    // flaky by nature) — this exists so the claim can be re-checked by hand.
    // Worst case for the old quadratic scan: zero overlap between the two
    // histories means every pre-compaction item forces a full scan of the
    // replacement array (`.find` never short-circuits on a match), so this
    // isolates the O(n*m) term as cleanly as possible.
    //
    // Measured on this machine, `cargo test -p ct-adapters --release --
    // --ignored --nocapture linear_scan_replaces_the_quadratic_one_it_replaced`,
    // 5,000 x 5,000 non-overlapping items: the fingerprint version in this
    // file completes in ~0.41s. Temporarily restoring the pre-CT-066
    // quadratic `compare` (the one with `replacement.iter().enumerate().find`)
    // and rerunning the identical test took ~4.52s -- an ~11x difference
    // already at this size, growing without bound as histories grow further
    // since one term is O(n) and the other O(n^2). See the CT-066 report for
    // the exact numbers from that run.
    #[test]
    #[ignore = "CT-066: manual verification of the linear-scan claim, not a CI assertion"]
    fn linear_scan_replaces_the_quadratic_one_it_replaced() {
        use std::time::Instant;

        fn synthetic_item(tag: &str, id: usize) -> Value {
            json!({
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": format!("{tag}-{id}-{}", "x".repeat(64)),
                }],
            })
        }

        const N: usize = 5_000;
        let source = SourceRef::new(FileId(0), 0, 500, 1);
        let before: Vec<LiveItem> = (0..N)
            .map(|i| LiveItem {
                value: synthetic_item("before", i),
                source,
            })
            .collect();
        let replacement: Vec<Value> = (0..N).map(|i| synthetic_item("after", i)).collect();

        let start = Instant::now();
        let items = compare(before, &replacement, source, &estimator());
        let elapsed = start.elapsed();

        assert_eq!(
            items.len(),
            2 * N,
            "every before-item drops and every replacement-item is added when nothing overlaps"
        );
        assert!(items[..N]
            .iter()
            .all(|item| matches!(item.disposition, CompactionItemDisposition::Dropped { .. })));
        assert!(items[N..].iter().all(|item| matches!(
            item.disposition,
            CompactionItemDisposition::AddedByReplacement { .. }
        )));

        eprintln!("CT-066 timing: {N}x{N} non-overlapping items compared in {elapsed:?}");
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
        assert_eq!(
            items[0].disposition,
            CompactionItemDisposition::Preserved {
                history_index: 0,
                replacement_index: 0,
            }
        );
        assert!(items[0].text_tokens.is_some());
        assert_eq!(
            items[1].disposition,
            CompactionItemDisposition::Dropped { history_index: 1 }
        );
        assert_eq!(items[1].item_type, "function_call_output");
        assert_eq!(
            items[2].disposition,
            CompactionItemDisposition::AddedByReplacement {
                replacement_index: 1,
            }
        );
        assert_eq!(items[2].item_type, "compaction");
        assert!(items[2].text_tokens.is_none());
    }

    #[test]
    fn a_preserved_item_records_where_it_moved_to_in_the_replacement_list() {
        // The replacement lists the opaque compaction blob first, so the
        // preserved message lands at a different index than it held in the
        // pre-compaction history: this is the case a bare `ordinal` field
        // could not represent.
        let events = vec![
            event(1, EventKind::Unrecognised, "response_item/message"),
            event(
                2,
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
                r#"{"type":"compacted","payload":{"replacement_history":[{"type":"compaction","encrypted_content":"opaque"},{"type":"message","role":"user","content":[{"type":"input_text","text":"keep"}]}]}}"#,
            ),
        ]);
        let report = diff(&session(events), &lines, &estimator());
        let CompactionDiff::Available { items, .. } = &report[0] else {
            panic!("expected diff")
        };
        assert_eq!(items.len(), 2);
        assert_eq!(
            items[0].disposition,
            CompactionItemDisposition::Preserved {
                history_index: 0,
                replacement_index: 1,
            }
        );
        assert_eq!(
            items[1].disposition,
            CompactionItemDisposition::AddedByReplacement {
                replacement_index: 0,
            }
        );
        assert_eq!(items[1].item_type, "compaction");
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
