//! Opt-in exact token counting for Codex.
//!
//! The default sizing path never loads content: adapters record a character
//! count at parse time and divide by a ratio. That is what makes a 55 MB
//! session tractable, and it is why every Codex item is `Estimated` unless
//! something asks for better.
//!
//! This module is that something. For each item it seeks to the line the item
//! came from, re-parses it, extracts the model-visible text, and runs the real
//! tokenizer over it. Codex is the only agent where this is worth doing:
//! `o200k_base` is the encoding its models actually use, so the result is a
//! measurement rather than a better guess.
//!
//! # What it deliberately will not count
//!
//! Roughly two Codex items in five hold something that is not tokenizable text
//! -- encrypted reasoning blobs, inline image data URLs, structured tool output
//! we would have to re-serialize before reading. Those keep their character
//! estimate. See [`Component`](super::parse::Component) for why counting them
//! anyway would be worse than not counting them.

use super::parse;
use crate::jsonl::MAX_PARSE_BYTES;
use ct_domain::ports::{ExactRecount, RawEventSource, TokenEstimator};
use ct_domain::ContextItem;
use serde_json::Value;

/// Re-count `items` in place, returning what was actually measured.
///
/// Never fails as a whole: an item whose line cannot be re-read keeps the
/// estimate it already had and is tallied under
/// [`ExactRecount::unavailable`]. Partial knowledge is the normal case here,
/// and reporting it beats discarding the items that did work.
pub fn recount(
    items: &mut [ContextItem],
    raw: &dyn RawEventSource,
    estimator: &dyn TokenEstimator,
) -> ExactRecount {
    let mut report = ExactRecount::default();

    for item in items.iter_mut() {
        let Some(source) = item.provenance.source else {
            report.unavailable += 1;
            continue;
        };

        // Oversized lines were deliberately not parsed during ingestion. Do
        // not defeat that memory bound during an exact recount: their narrow
        // scan already established that some content is non-text or only
        // partially measurable.
        if source.byte_len as usize > MAX_PARSE_BYTES {
            report.opaque += 1;
            continue;
        }

        let Ok(line) = raw.fetch(source) else {
            report.unavailable += 1;
            continue;
        };

        match text_of_line(&line) {
            Some(Countable::Text(text)) => {
                let counted = estimator.count_text(&text);
                // Only a trustworthy result is a recount. A caller that passed
                // a heuristic estimator gets its items left alone rather than
                // relabelled, so `counted` cannot overstate what happened.
                if counted.is_trustworthy() {
                    item.tokens = counted;
                    report.counted += 1;
                } else {
                    report.unavailable += 1;
                }
            }
            Some(Countable::Opaque) => report.opaque += 1,
            None => report.unavailable += 1,
        }
    }

    report
}

/// What one raw line offers an exact counter.
enum Countable {
    Text(String),
    /// The line is a context-occupying item, but part of it is not text we can
    /// tokenize honestly.
    Opaque,
}

/// Extract the model-visible text of one raw session line.
///
/// Dispatches on the envelope's own `type`, mirroring the parser, so lines that
/// are not replayed items -- `compacted`, `event_msg`, lifecycle records -- are
/// never counted. That matters more than it looks: a `compacted` item is sized
/// from its line's byte length as an explicit proxy, and tokenizing its
/// `replacement_history` would silently replace that proxy with a number
/// measuring something else.
fn text_of_line(line: &str) -> Option<Countable> {
    let value: Value = serde_json::from_str(line.trim()).ok()?;
    let payload = value.get("payload")?;

    match value.get("type").and_then(Value::as_str)? {
        "response_item" => match parse::content_text(payload) {
            Some(text) => Some(Countable::Text(text)),
            None => Some(Countable::Opaque),
        },
        "session_meta" => {
            let instructions = payload.get("base_instructions")?;
            parse::instruction_text(instructions).map(Countable::Text)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokenizers::{HeuristicEstimator, TiktokenEstimator};
    use ct_domain::ports::{PortResult, RawEventSource};
    use ct_domain::{
        ContextCategory, ContextItemId, ContextSource, FileId, Provenance, SourceRef, TokenCount,
    };

    /// Serves canned lines by line number, so these tests exercise the
    /// re-read/re-parse logic without touching a filesystem.
    struct Lines(Vec<&'static str>);

    impl RawEventSource for Lines {
        fn fetch(&self, source: SourceRef) -> PortResult<String> {
            Ok(self.0[source.line_no as usize - 1].to_string())
        }
    }

    struct MustNotFetch;

    impl RawEventSource for MustNotFetch {
        fn fetch(&self, _source: SourceRef) -> PortResult<String> {
            panic!("an oversized line must not be fetched for exact recounting")
        }
    }

    fn item(line: u32) -> ContextItem {
        ContextItem {
            id: ContextItemId::new(format!("codex:{line}")),
            category: ContextCategory::ToolOutputs,
            label: "x".into(),
            source: ContextSource::Unknown,
            tokens: TokenCount::estimated(9999),
            first_seen_turn: None,
            provenance: Provenance::observed(SourceRef::new(FileId(0), 0, 0, line)),
            preview: None,
            content_measurement: None,
        }
    }

    fn tiktoken() -> TiktokenEstimator {
        TiktokenEstimator::o200k().expect("o200k_base must load")
    }

    #[test]
    fn a_plain_text_message_is_measured_not_estimated() {
        let lines = Lines(vec![
            r#"{"type":"response_item","payload":{"type":"message","role":"user",
               "content":[{"type":"input_text","text":"hello world"}]}}"#,
        ]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &tiktoken());

        assert_eq!(report.counted, 1);
        assert!(items[0].tokens.is_trustworthy());
        assert!(
            items[0].tokens.tokens() < 10,
            "got {}",
            items[0].tokens.tokens()
        );
    }

    #[test]
    fn an_encrypted_reasoning_item_keeps_its_estimate() {
        // The blob occupies context and its length counts, but tokenizing the
        // ciphertext would measure our transport encoding, not the model's input.
        let lines = Lines(vec![
            r#"{"type":"response_item","payload":{"type":"reasoning",
               "summary":[{"type":"summary_text","text":"thinking"}],
               "encrypted_content":"gAAAAABm...."}}"#,
        ]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &tiktoken());

        assert_eq!(report.opaque, 1);
        assert_eq!(report.counted, 0);
        assert_eq!(
            items[0].tokens.tokens(),
            9999,
            "the estimate must survive untouched"
        );
    }

    #[test]
    fn an_oversized_item_is_refused_before_the_raw_line_is_fetched() {
        let mut oversized = item(1);
        oversized.provenance = Provenance::observed(SourceRef::new(
            FileId(0),
            0,
            (MAX_PARSE_BYTES + 1) as u32,
            1,
        ));
        let mut items = vec![oversized];

        let report = recount(&mut items, &MustNotFetch, &tiktoken());

        assert_eq!(report.opaque, 1);
        assert_eq!(report.counted, 0);
        assert_eq!(items[0].tokens.tokens(), 9999);
    }

    #[test]
    fn structured_tool_output_is_not_counted_from_our_own_reserialization() {
        let lines = Lines(vec![
            r#"{"type":"response_item","payload":{"type":"function_call_output",
               "call_id":"c1","output":{"content":"ok","metadata":{"exit_code":0}}}}"#,
        ]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &tiktoken());

        assert_eq!(report.opaque, 1);
        assert_eq!(items[0].tokens.tokens(), 9999);
    }

    #[test]
    fn lines_that_are_not_replayed_items_are_never_counted() {
        // A compaction item is sized from its line's byte length as a declared
        // proxy. Tokenizing `replacement_history` would quietly swap that proxy
        // for a measurement of something else.
        let lines = Lines(vec![
            r#"{"type":"compacted","payload":{"replacement_history":[{"role":"user",
               "content":"summary text"}]}}"#,
        ]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &tiktoken());

        assert_eq!(report.unavailable, 1);
        assert_eq!(items[0].tokens.tokens(), 9999);
    }

    #[test]
    fn the_system_prompt_is_counted_from_session_meta() {
        let lines = Lines(vec![
            r#"{"type":"session_meta","payload":{"id":"s1",
               "base_instructions":"You are Codex, a coding agent."}}"#,
        ]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &tiktoken());

        assert_eq!(report.counted, 1);
        assert!(items[0].tokens.is_trustworthy());
    }

    #[test]
    fn a_heuristic_estimator_cannot_launder_a_guess_into_a_recount() {
        let lines = Lines(vec![
            r#"{"type":"response_item","payload":{"type":"message","role":"user",
               "content":[{"type":"input_text","text":"hello world"}]}}"#,
        ]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &HeuristicEstimator::for_prose());

        assert_eq!(report.counted, 0, "a heuristic result is not a measurement");
        assert_eq!(report.unavailable, 1);
        assert_eq!(items[0].tokens.tokens(), 9999);
    }

    #[test]
    fn an_unreadable_line_leaves_its_item_alone() {
        let lines = Lines(vec!["{ this is not json"]);
        let mut items = vec![item(1)];
        let report = recount(&mut items, &lines, &tiktoken());

        assert_eq!(report.unavailable, 1);
        assert_eq!(report.total(), 1);
        assert_eq!(items[0].tokens.tokens(), 9999);
    }
}
