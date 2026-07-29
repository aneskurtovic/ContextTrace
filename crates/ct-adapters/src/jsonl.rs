//! Streaming JSONL reader shared by both agent adapters.
//!
//! Both supported agents write one JSON object per line, so the mechanics of
//! walking a session file are agent-independent even though the schemas are
//! not. This module owns those mechanics; the anti-corruption layers own the
//! meaning.
//!
//! # The oversized-line problem
//!
//! Session lines are not uniformly small. Codex embeds base64 images directly
//! in `compacted` payloads, producing single lines of many megabytes inside
//! files that reach 55 MB. Parsing every line into a `serde_json::Value`
//! unconditionally would spend enormous time and memory on exactly the lines we
//! learn the least from.
//!
//! So lines above [`MAX_PARSE_BYTES`] are not fully parsed. We sniff their type
//! from a bounded prefix and let the receiving adapter inspect the raw bytes
//! before this reader reuses its buffer. That permits narrow, allocation-free
//! recovery of context metadata without building a multi-megabyte JSON tree.

use ct_domain::ports::{PortError, PortResult};
use serde_json::Value;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Lines larger than this are sniffed rather than parsed.
///
/// 4 MiB comfortably exceeds ordinary textual events while excluding the
/// inline-image payloads that make up the pathological cases.
pub const MAX_PARSE_BYTES: usize = 4 * 1024 * 1024;

/// How much of an oversized line to inspect when sniffing its type.
const SNIFF_BYTES: usize = 16 * 1024;

/// One line of a session file, with its position recorded.
pub struct LineRecord {
    /// Byte offset of the line start within the file.
    pub offset: u64,
    /// Byte length of the line, excluding the trailing newline.
    pub len: u32,
    /// 1-based line number.
    pub line_no: u32,
    /// The parsed object, or `None` when the line was too large to parse or was
    /// not valid JSON.
    pub value: Option<Value>,
    /// True when the line was skipped for size rather than being malformed.
    pub oversized: bool,
    /// Type string recovered by prefix sniffing when `value` is `None`.
    pub sniffed_type: Option<String>,
}

impl LineRecord {
    /// The line's `type` field, whether parsed or sniffed.
    pub fn type_str(&self) -> Option<&str> {
        match &self.value {
            Some(v) => v.get("type").and_then(Value::as_str),
            None => self.sniffed_type.as_deref(),
        }
    }
}

/// Read a JSONL file, yielding one [`LineRecord`] per line.
///
/// Never fails on a bad line: malformed JSON yields a record with `value:
/// None`, which adapters translate into an unrecognised event. A single corrupt
/// line must not cost the user the rest of a session.
pub fn read_lines(path: &Path, mut visit: impl FnMut(LineRecord, &[u8])) -> PortResult<()> {
    let file = File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
    // 1 MiB buffer: these files are large and read strictly sequentially.
    let mut reader = BufReader::with_capacity(1024 * 1024, file);

    let mut buf: Vec<u8> = Vec::with_capacity(64 * 1024);
    let mut offset: u64 = 0;
    let mut line_no: u32 = 0;

    loop {
        buf.clear();
        let read = reader
            .read_until(b'\n', &mut buf)
            .map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
        if read == 0 {
            break;
        }
        line_no += 1;

        // Strip the newline (and a Windows carriage return) without copying.
        let mut content = &buf[..read];
        if content.ends_with(b"\n") {
            content = &content[..content.len() - 1];
        }
        if content.ends_with(b"\r") {
            content = &content[..content.len() - 1];
        }

        let line_offset = offset;
        offset += read as u64;

        if content.is_empty() {
            continue;
        }

        let record = if content.len() > MAX_PARSE_BYTES {
            LineRecord {
                offset: line_offset,
                len: content.len().min(u32::MAX as usize) as u32,
                line_no,
                value: None,
                oversized: true,
                sniffed_type: sniff_type(&content[..SNIFF_BYTES.min(content.len())]),
            }
        } else {
            LineRecord {
                offset: line_offset,
                len: content.len().min(u32::MAX as usize) as u32,
                line_no,
                value: serde_json::from_slice(content).ok(),
                oversized: false,
                sniffed_type: None,
            }
        };

        visit(record, content);
    }

    Ok(())
}

/// Recover a `"type":"..."` value from the head of an unparsed line.
///
/// A deliberately dumb scan rather than a streaming JSON parser: both agents put
/// `type` near the front of the object, and being wrong here costs a
/// reclassification to "unrecognised", not incorrect output.
fn sniff_type(head: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(head);
    let key = "\"type\"";
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let colon = rest.find(':')?;
    let after = rest[colon + 1..].trim_start();
    let quoted = after.strip_prefix('"')?;
    let end = quoted.find('"')?;
    Some(quoted[..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("ct-jsonl-test-{name}"));
        let mut f = File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        path
    }

    #[test]
    fn records_offsets_and_line_numbers() {
        let path = temp_file("offsets.jsonl", b"{\"type\":\"a\"}\n{\"type\":\"b\"}\n");
        let mut seen = Vec::new();
        read_lines(&path, |r, _| {
            seen.push((r.line_no, r.offset, r.len, r.type_str().map(String::from)))
        })
        .unwrap();

        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0], (1, 0, 12, Some("a".into())));
        assert_eq!(seen[1], (2, 13, 12, Some("b".into())));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_corrupt_line_does_not_abort_the_file() {
        let path = temp_file(
            "corrupt.jsonl",
            b"{\"type\":\"good\"}\nNOT JSON AT ALL\n{\"type\":\"also-good\"}\n",
        );
        let mut kinds = Vec::new();
        read_lines(&path, |r, _| kinds.push(r.type_str().map(String::from))).unwrap();

        assert_eq!(
            kinds,
            vec![Some("good".into()), None, Some("also-good".into())],
            "a bad line must cost that line only"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn blank_lines_are_skipped_but_still_advance_offsets() {
        let path = temp_file("blank.jsonl", b"{\"type\":\"a\"}\n\n{\"type\":\"b\"}\n");
        let mut seen = Vec::new();
        read_lines(&path, |r, _| seen.push((r.line_no, r.offset))).unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1], (3, 14), "offset must account for the blank line");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn crlf_endings_are_not_counted_as_content() {
        let path = temp_file("crlf.jsonl", b"{\"type\":\"a\"}\r\n");
        let mut lens = Vec::new();
        read_lines(&path, |r, _| lens.push(r.len)).unwrap();
        assert_eq!(lens, vec![12], "CR and LF are framing, not payload");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn oversized_lines_are_sniffed_not_parsed() {
        let filler = "x".repeat(MAX_PARSE_BYTES + 1024);
        let line = format!("{{\"type\":\"compacted\",\"blob\":\"{filler}\"}}\n");
        let path = temp_file("oversized.jsonl", line.as_bytes());

        let mut seen = Vec::new();
        read_lines(&path, |r, raw| {
            assert_eq!(raw.len(), r.len as usize);
            seen.push((
                r.oversized,
                r.value.is_none(),
                r.type_str().map(String::from),
                r.len,
            ))
        })
        .unwrap();

        assert_eq!(seen.len(), 1);
        let (oversized, unparsed, ty, len) = &seen[0];
        assert!(oversized, "line beyond the cap must be flagged");
        assert!(unparsed, "and must not be fully parsed");
        assert_eq!(
            ty.as_deref(),
            Some("compacted"),
            "but its type is still recovered"
        );
        assert!(
            *len as usize > MAX_PARSE_BYTES,
            "its true size is still recorded"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sniffing_tolerates_whitespace_and_missing_types() {
        assert_eq!(sniff_type(b"{ \"type\" : \"x\" }"), Some("x".into()));
        assert_eq!(sniff_type(b"{\"other\":1}"), None);
        assert_eq!(sniff_type(b"{\"type\":123}"), None);
    }
}
