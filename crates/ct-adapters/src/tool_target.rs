//! What a tool call acted on, taken from the call's own arguments.
//!
//! Both agents log a tool call as a name plus an arguments object. The name
//! alone makes a context breakdown nearly useless at the point it matters
//! most: a turn with four `Read` results, at 14,805 and 7,822 and 2,745 and
//! 2,090 tokens, says which *tool* cost the context and not which *file*. The
//! argument that identifies the target is right there in the log and was simply
//! being discarded at parse time.
//!
//! # Why a key list rather than per-tool knowledge
//!
//! Encoding "Read takes file_path, Bash takes command, Grep takes pattern"
//! would mean this file needs editing every time either agent ships a tool, and
//! silently degrading for MCP tools nobody here has heard of. The keys below
//! are instead ordered by how *specifically* each identifies a target, and the
//! first one present wins. An unknown tool that happens to take a `path` or a
//! `query` is named correctly without this module knowing it exists.
//!
//! When no key matches, the answer is `None`. Naming a call after some other
//! field of its arguments would be inventing a label, and a wrong filename is
//! worse than no filename.

use serde_json::Value;

/// Argument names that identify what a call acted on, most specific first.
///
/// `file_path` beats `path` because agents that use both mean the file by the
/// former; `command` beats `pattern` because a shell call is identified by what
/// it ran; `description` and `prompt` are last, being prose rather than an
/// address.
const TARGET_KEYS: [&str; 10] = [
    "file_path",
    "notebook_path",
    "path",
    "command",
    "pattern",
    "url",
    "query",
    "file",
    "description",
    "prompt",
];

/// How much of a target to keep in the parsed event.
///
/// A **storage** bound, not a display one: a heredoc pasted into a shell call
/// runs to kilobytes, and events hold lengths rather than payloads by design.
/// Deliberately far wider than any column, so that shortening for a terminal is
/// the presentation layer's business and a row never shows two ellipses -- one
/// from here and one from the renderer.
const MAX_CHARS: usize = 160;

/// Describe what a tool call targeted, or `None` when its arguments do not say.
pub fn describe(input: &Value) -> Option<String> {
    let object = input.as_object()?;
    for key in TARGET_KEYS {
        if let Some(value) = object.get(key) {
            if let Some(text) = stringify(value) {
                return Some(text);
            }
        }
    }
    None
}

/// Describe a call whose arguments were logged as a JSON string.
///
/// Codex stores `arguments` as an encoded string rather than an object. When it
/// does not parse as JSON it is used literally, because a shell command sent as
/// a bare string is still the best available name for the call.
pub fn describe_encoded(arguments: &str) -> Option<String> {
    match serde_json::from_str::<Value>(arguments) {
        Ok(value) => describe(&value),
        Err(_) => stringify(&Value::String(arguments.to_string())),
    }
}

/// What a call id resolves to: the tool's name, and what it acted on.
///
/// Both adapters build this index, because a tool *result* records only the id
/// of the call it answers -- the name and the target live on the call.
pub type CallIndex<'a> = std::collections::HashMap<&'a str, (&'a str, Option<&'a str>)>;

/// Compose a context-item label from a tool and its target.
///
/// Shared so both adapters name their rows the same way. No `Tool output:`
/// prefix: every view showing a label shows the item's category beside it, so a
/// prefix would spend a third of the column restating "Tool outputs" — and the
/// call-versus-result distinction it used to carry is what the category *is*.
///
/// Falls back to the bare tool name rather than a placeholder: `TodoWrite` is
/// honest, `TodoWrite (unknown)` is noise.
pub fn label(tool: &str, target: Option<&str>) -> String {
    match target {
        Some(t) => format!("{tool} {t}"),
        None => tool.to_string(),
    }
}

/// Render one argument value as a single short line.
fn stringify(value: &Value) -> Option<String> {
    let raw = match value {
        Value::String(s) => s.clone(),
        // Codex logs shell commands as `["bash", "-lc", "ls -la"]`. Joining
        // keeps the literal invocation; picking out the "interesting" element
        // would be guessing at a convention neither agent documents.
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str())
            .collect::<Vec<_>>()
            .join(" "),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };

    let collapsed = collapse_whitespace(&raw);
    if collapsed.is_empty() {
        return None;
    }
    Some(truncate(&collapsed, MAX_CHARS))
}

/// Flatten newlines and runs of spaces, so a multi-line command stays one row.
fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let kept: String = s.chars().take(max).collect();
    format!("{kept}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_file_read_is_named_by_its_path() {
        let input = json!({"file_path": "C:\\src\\schema.ts", "offset": 40});
        assert_eq!(describe(&input).as_deref(), Some("C:\\src\\schema.ts"));
    }

    #[test]
    fn the_more_specific_key_wins() {
        // Some calls carry both. `file_path` is the file; `path` is the
        // directory it was searched in.
        let input = json!({"path": "src/", "file_path": "src/main.rs"});
        assert_eq!(describe(&input).as_deref(), Some("src/main.rs"));
    }

    #[test]
    fn a_shell_command_survives_being_multi_line() {
        let input = json!({"command": "cargo test \\\n  --workspace"});
        assert_eq!(
            describe(&input).as_deref(),
            Some("cargo test \\ --workspace")
        );
    }

    #[test]
    fn codex_command_arrays_keep_the_literal_invocation() {
        let input = json!({"command": ["bash", "-lc", "ls -la"]});
        assert_eq!(describe(&input).as_deref(), Some("bash -lc ls -la"));
    }

    #[test]
    fn codex_arguments_arrive_as_an_encoded_string() {
        let encoded = r#"{"command":["bash","-lc","cargo build"]}"#;
        assert_eq!(
            describe_encoded(encoded).as_deref(),
            Some("bash -lc cargo build")
        );
    }

    #[test]
    fn arguments_that_are_not_json_are_used_as_written() {
        assert_eq!(describe_encoded("ls -la").as_deref(), Some("ls -la"));
    }

    #[test]
    fn an_unknown_tool_is_still_named_when_it_takes_a_known_key() {
        // The point of a key list rather than per-tool knowledge: this module
        // has never heard of `mcp__linear__search`.
        let input = json!({"query": "assigned to me"});
        assert_eq!(describe(&input).as_deref(), Some("assigned to me"));
    }

    #[test]
    fn arguments_that_name_nothing_yield_nothing() {
        // TodoWrite and friends. A label invented from some other field would
        // be worse than no label.
        assert_eq!(describe(&json!({"todos": [{"content": "x"}]})), None);
        assert_eq!(describe(&json!({})), None);
        assert_eq!(describe(&json!("not an object")), None);
    }

    #[test]
    fn an_empty_value_is_not_a_name() {
        assert_eq!(describe(&json!({"file_path": "   "})), None);
    }

    #[test]
    fn long_targets_are_truncated_on_a_character_boundary() {
        let long = "a".repeat(200);
        let out = describe(&json!({ "command": long })).unwrap();
        assert_eq!(
            out.chars().count(),
            MAX_CHARS + 1,
            "64 chars plus an ellipsis"
        );
        assert!(out.ends_with('\u{2026}'));

        let multibyte = "é".repeat(200);
        let out = describe(&json!({ "command": multibyte })).unwrap();
        assert_eq!(out.chars().count(), MAX_CHARS + 1);
    }
}
