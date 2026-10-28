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
//! from a bounded prefix. The callback receives at most the parse budget plus
//! two framing bytes, with `truncated` set when the rest was drained. Adapters
//! must report incomplete records rather than infer facts from a prefix.

use ct_domain::ports::{PortError, PortResult};
use serde::de::{IgnoredAny, MapAccess, Visitor};
use serde::Deserializer;
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
    /// Only a bounded prefix is available to the callback. Never interpret it
    /// as a complete JSON record or use it to estimate the missing payload.
    pub truncated: bool,
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
        let (read, framing) = read_bounded_line(&mut reader, &mut buf)
            .map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
        if read == 0 {
            break;
        }
        line_no = line_no.saturating_add(1);

        // Strip the newline (and a Windows carriage return) without copying.
        let content_len = read - framing;
        buf.truncate(content_len.min(buf.len() as u64) as usize);
        let content = &buf[..];

        let line_offset = offset;
        offset = offset.saturating_add(read);

        if content.is_empty() {
            continue;
        }

        let record = if content_len > MAX_PARSE_BYTES as u64 {
            LineRecord {
                offset: line_offset,
                len: content_len.min(u32::MAX as u64) as u32,
                line_no,
                value: None,
                oversized: true,
                truncated: content_len > content.len() as u64,
                sniffed_type: sniff_type(&content[..SNIFF_BYTES.min(content.len())]),
            }
        } else {
            LineRecord {
                offset: line_offset,
                len: content.len().min(u32::MAX as usize) as u32,
                line_no,
                value: serde_json::from_slice(content).ok(),
                oversized: false,
                truncated: false,
                sniffed_type: None,
            }
        };

        visit(record, content);
    }

    Ok(())
}

/// Drain an arbitrarily long line while retaining at most the parse budget
/// plus CRLF framing. The buffer cannot grow with the input line's length.
fn read_bounded_line(reader: &mut impl BufRead, buf: &mut Vec<u8>) -> std::io::Result<(u64, u64)> {
    let mut total = 0u64;
    let mut last = None;
    let mut previous = None;
    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let used = chunk
            .iter()
            .position(|b| *b == b'\n')
            .map_or(chunk.len(), |i| i + 1);
        let keep = used.min((MAX_PARSE_BYTES + 2).saturating_sub(buf.len()));
        buf.extend_from_slice(&chunk[..keep]);
        for &byte in &chunk[used.saturating_sub(2)..used] {
            previous = last;
            last = Some(byte);
        }
        total = total.saturating_add(used as u64);
        let done = last == Some(b'\n');
        reader.consume(used);
        if done {
            break;
        }
    }
    let framing = u64::from(last == Some(b'\n'))
        + u64::from(if last == Some(b'\n') {
            previous == Some(b'\r')
        } else {
            last == Some(b'\r')
        });
    Ok((total, framing))
}

/// Recover a `"type":"..."` value from the head of an unparsed line.
///
/// Inspect only top-level keys; nested `type` fields and quoted lookalikes do
/// not identify an envelope. The recovered type is a diagnostic, not evidence
/// that the rest of an oversized record was valid JSON.
fn sniff_type(head: &[u8]) -> Option<String> {
    struct TypeVisitor<'a>(&'a mut Option<String>);
    impl<'de> Visitor<'de> for TypeVisitor<'_> {
        type Value = Option<String>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a JSON object")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
            while let Some(key) = map.next_key::<String>()? {
                if key == "type" {
                    *self.0 = Some(map.next_value::<String>()?);
                    // Stop before the incomplete remainder. deserialize_map
                    // otherwise insists on the closing brace after visit_map.
                    return Err(serde::de::Error::custom("type recovered"));
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(None)
        }
    }
    let mut found = None;
    let _ = serde_json::Deserializer::from_slice(head).deserialize_map(TypeVisitor(&mut found));
    found
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
            assert!(r.truncated);
            assert!(raw.len() <= MAX_PARSE_BYTES + 2);
            assert!(raw.len() < r.len as usize);
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
        assert_eq!(
            sniff_type(br#"{"nested":{"type":"wrong"},"type":"outer"}"#),
            Some("outer".into())
        );
        assert_eq!(
            sniff_type(br#"{"text":"\"type\":\"wrong\"","type":"outer"}"#),
            Some("outer".into())
        );
    }

    #[test]
    fn huge_line_is_drained_and_the_next_record_keeps_its_position() {
        let path = temp_file("drained.jsonl", b"");
        let mut file = File::create(&path).unwrap();
        let chunk = vec![b'x'; 64 * 1024];
        for _ in 0..160 {
            file.write_all(&chunk).unwrap();
        }
        file.write_all(b"\r\n{\"type\":\"after\"}").unwrap();
        drop(file);
        let mut seen = Vec::new();
        read_lines(&path, |r, raw| {
            assert!(raw.len() <= MAX_PARSE_BYTES + 2);
            seen.push((
                r.offset,
                r.len,
                r.line_no,
                r.truncated,
                r.type_str().map(str::to_owned),
            ));
        })
        .unwrap();
        assert_eq!(seen[0].1, 10 * 1024 * 1024);
        assert_eq!(
            seen[1],
            (10 * 1024 * 1024 + 2, 16, 2, false, Some("after".into()))
        );
        let _ = std::fs::remove_file(path);
    }
}
