//! Local secret detection and redaction.
//!
//! Findings deliberately contain only a kind and a location. The matched value
//! never enters a report type, so neither a renderer nor a future serializer
//! can accidentally disclose it. Scanning is likewise opt-in: ordinary session
//! inspection does not re-read raw content.

use crate::ContextTrace;
use ct_domain::ports::RawEventSource;
use ct_domain::{AgentSession, ContextSource, SourceRef, TurnNumber};
use std::borrow::Cow;
use std::collections::{BTreeMap, HashSet};

/// A high-confidence credential shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SecretKind {
    OpenAiApiKey,
    AnthropicApiKey,
    GitHubToken,
    AwsAccessKey,
    GoogleApiKey,
    SlackToken,
    StripeLiveKey,
    BearerToken,
    PrivateKey,
    EnvironmentSecret,
}

impl SecretKind {
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::OpenAiApiKey => "OpenAI API key",
            SecretKind::AnthropicApiKey => "Anthropic API key",
            SecretKind::GitHubToken => "GitHub token",
            SecretKind::AwsAccessKey => "AWS access key id",
            SecretKind::GoogleApiKey => "Google API key",
            SecretKind::SlackToken => "Slack token",
            SecretKind::StripeLiveKey => "Stripe live secret",
            SecretKind::BearerToken => "Bearer token",
            SecretKind::PrivateKey => "private key",
            SecretKind::EnvironmentSecret => "secret-like assignment",
        }
    }

    pub fn marker(self) -> &'static str {
        match self {
            SecretKind::OpenAiApiKey => "openai-api-key",
            SecretKind::AnthropicApiKey => "anthropic-api-key",
            SecretKind::GitHubToken => "github-token",
            SecretKind::AwsAccessKey => "aws-access-key-id",
            SecretKind::GoogleApiKey => "google-api-key",
            SecretKind::SlackToken => "slack-token",
            SecretKind::StripeLiveKey => "stripe-live-secret",
            SecretKind::BearerToken => "bearer-token",
            SecretKind::PrivateKey => "private-key",
            SecretKind::EnvironmentSecret => "environment-secret",
        }
    }
}

/// A non-sensitive finding safe to show in the terminal.
///
/// There is intentionally no matched-text field and no `Serialize` derive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretFinding {
    pub kind: SecretKind,
    pub occurrences: usize,
    pub turn: Option<TurnNumber>,
    pub line_no: u32,
    pub event_type: String,
}

/// Result of a read-only session scan.
///
/// This type intentionally is not serializable. Secret scanning is a terminal
/// diagnostic, not another export surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SecretScanReport {
    pub findings: Vec<SecretFinding>,
    pub scanned_records: usize,
    pub unreadable_records: usize,
}

impl SecretScanReport {
    pub fn occurrence_count(&self) -> usize {
        self.findings.iter().map(|f| f.occurrences).sum()
    }
}

/// Secret handling requested for an export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ExportRedaction {
    #[default]
    None,
    Secrets,
}

impl ExportRedaction {
    pub fn label(self) -> &'static str {
        match self {
            ExportRedaction::None => "none",
            ExportRedaction::Secrets => "secrets",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportReport {
    pub redactions: usize,
}

#[derive(Debug, Clone, Copy)]
struct SecretMatch {
    kind: SecretKind,
    start: usize,
    end: usize,
}

/// Redact every recognised value while preserving all surrounding text.
pub(crate) fn redact_text(text: &str) -> (Cow<'_, str>, usize) {
    let matches = find_secrets(text);
    if matches.is_empty() {
        return (Cow::Borrowed(text), 0);
    }

    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    for found in &matches {
        out.push_str(&text[cursor..found.start]);
        out.push_str("[REDACTED:");
        out.push_str(found.kind.marker());
        out.push(']');
        cursor = found.end;
    }
    out.push_str(&text[cursor..]);
    (Cow::Owned(out), matches.len())
}

/// Redact the string-bearing parts of a source without changing its wire shape.
pub(crate) fn redact_source(source: &ContextSource) -> (Cow<'_, ContextSource>, usize) {
    match source {
        ContextSource::InstructionFile { path } => {
            let (path, count) = redact_text(path);
            match path {
                Cow::Borrowed(_) => (Cow::Borrowed(source), 0),
                Cow::Owned(path) => (Cow::Owned(ContextSource::InstructionFile { path }), count),
            }
        }
        ContextSource::ProjectConfig { path: Some(path) } => {
            let (path, count) = redact_text(path);
            match path {
                Cow::Borrowed(_) => (Cow::Borrowed(source), 0),
                Cow::Owned(path) => (
                    Cow::Owned(ContextSource::ProjectConfig { path: Some(path) }),
                    count,
                ),
            }
        }
        ContextSource::HarnessInjection { mechanism } => {
            let (mechanism, count) = redact_text(mechanism);
            match mechanism {
                Cow::Borrowed(_) => (Cow::Borrowed(source), 0),
                Cow::Owned(mechanism) => (
                    Cow::Owned(ContextSource::HarnessInjection { mechanism }),
                    count,
                ),
            }
        }
        ContextSource::ToolExecution { tool } => {
            let (tool, count) = redact_text(tool);
            match tool {
                Cow::Borrowed(_) => (Cow::Borrowed(source), 0),
                Cow::Owned(tool) => (Cow::Owned(ContextSource::ToolExecution { tool }), count),
            }
        }
        ContextSource::FileRead { path } => {
            let (path, count) = redact_text(path);
            match path {
                Cow::Borrowed(_) => (Cow::Borrowed(source), 0),
                Cow::Owned(path) => (Cow::Owned(ContextSource::FileRead { path }), count),
            }
        }
        _ => (Cow::Borrowed(source), 0),
    }
}

impl ContextTrace {
    /// Scan context-bearing raw records without retaining or returning values.
    ///
    /// Records are scanned once even when an item survives for hundreds of
    /// turns. Compaction records are included because Codex can place their
    /// replacement history back into context; the base-instructions record is
    /// included separately because its event is session metadata rather than a
    /// normal context item.
    pub fn scan_secrets(
        &self,
        session: &AgentSession,
        raw: &dyn RawEventSource,
    ) -> SecretScanReport {
        let mut report = SecretScanReport::default();
        let mut seen = HashSet::new();

        for event in session.events() {
            let context_bearing = event.occupies_context()
                || matches!(
                    event.kind,
                    ct_domain::EventKind::Compacted(ref facts) if facts.replacement_recorded
                );
            if !context_bearing || !seen.insert(source_key(event.source)) {
                continue;
            }
            scan_source(raw, event.source, event.turn, &event.raw_type, &mut report);
        }

        if let Some(source) = session.metadata().base_instructions {
            if seen.insert(source_key(source)) {
                scan_source(
                    raw,
                    source,
                    None,
                    "session_meta/base_instructions",
                    &mut report,
                );
            }
        }

        report
    }
}

fn scan_source(
    raw: &dyn RawEventSource,
    source: SourceRef,
    turn: Option<TurnNumber>,
    event_type: &str,
    report: &mut SecretScanReport,
) {
    let Ok(text) = raw.fetch(source) else {
        report.unreadable_records += 1;
        return;
    };
    report.scanned_records += 1;

    let mut counts = BTreeMap::new();
    for found in find_secrets(&text) {
        *counts.entry(found.kind).or_insert(0usize) += 1;
    }
    report
        .findings
        .extend(counts.into_iter().map(|(kind, occurrences)| SecretFinding {
            kind,
            occurrences,
            turn,
            line_no: source.line_no,
            event_type: event_type.to_string(),
        }));
}

fn source_key(source: SourceRef) -> (u32, u64, u32, u32) {
    (
        source.file.0,
        source.byte_offset,
        source.byte_len,
        source.line_no,
    )
}

fn find_secrets(text: &str) -> Vec<SecretMatch> {
    let mut found = Vec::new();

    find_prefixed(
        text,
        "sk-ant-",
        20,
        180,
        token_char,
        SecretKind::AnthropicApiKey,
        &mut found,
    );
    for prefix in ["sk-proj-", "sk-svcacct-"] {
        find_prefixed(
            text,
            prefix,
            20,
            240,
            token_char,
            SecretKind::OpenAiApiKey,
            &mut found,
        );
    }
    find_openai_legacy(text, &mut found);

    for prefix in ["ghp_", "gho_", "ghu_", "ghs_", "ghr_"] {
        find_prefixed(
            text,
            prefix,
            36,
            255,
            token_char,
            SecretKind::GitHubToken,
            &mut found,
        );
    }
    find_prefixed(
        text,
        "github_pat_",
        22,
        255,
        token_char,
        SecretKind::GitHubToken,
        &mut found,
    );
    for prefix in ["AKIA", "ASIA"] {
        find_fixed(
            text,
            prefix,
            16,
            |b| b.is_ascii_uppercase() || b.is_ascii_digit(),
            SecretKind::AwsAccessKey,
            &mut found,
        );
    }
    find_fixed(
        text,
        "AIza",
        35,
        token_char,
        SecretKind::GoogleApiKey,
        &mut found,
    );
    for prefix in ["xoxb-", "xoxp-", "xoxa-", "xoxr-", "xoxs-"] {
        find_prefixed(
            text,
            prefix,
            16,
            180,
            token_char,
            SecretKind::SlackToken,
            &mut found,
        );
    }
    for prefix in ["sk_live_", "rk_live_"] {
        find_prefixed(
            text,
            prefix,
            16,
            180,
            token_char,
            SecretKind::StripeLiveKey,
            &mut found,
        );
    }
    for prefix in ["Bearer ", "bearer "] {
        find_prefixed(
            text,
            prefix,
            20,
            2048,
            bearer_char,
            SecretKind::BearerToken,
            &mut found,
        );
    }
    find_private_keys(text, &mut found);
    find_secret_assignments(text, &mut found);

    // Prefer the longest match at a shared start, then discard overlaps. This
    // keeps an Anthropic `sk-ant-...` from also becoming a generic OpenAI key.
    found.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)));
    let mut disjoint = Vec::with_capacity(found.len());
    for candidate in found {
        if disjoint
            .last()
            .is_some_and(|previous: &SecretMatch| candidate.start < previous.end)
        {
            continue;
        }
        disjoint.push(candidate);
    }
    disjoint
}

fn find_openai_legacy(text: &str, found: &mut Vec<SecretMatch>) {
    for (start, _) in text.match_indices("sk-") {
        if start > 0 && text.as_bytes()[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let suffix = &text.as_bytes()[start + 3..];
        if suffix.starts_with(b"ant-")
            || suffix.starts_with(b"proj-")
            || suffix.starts_with(b"svcacct-")
        {
            continue;
        }
        let len = suffix.iter().take_while(|b| token_char(**b)).count();
        if len >= 20 {
            found.push(SecretMatch {
                kind: SecretKind::OpenAiApiKey,
                start,
                end: start + 3 + len.min(180),
            });
        }
    }
}

fn find_prefixed(
    text: &str,
    prefix: &str,
    min_suffix: usize,
    max_suffix: usize,
    allowed: fn(u8) -> bool,
    kind: SecretKind,
    found: &mut Vec<SecretMatch>,
) {
    for (start, _) in text.match_indices(prefix) {
        if start > 0 && text.as_bytes()[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let suffix = &text.as_bytes()[start + prefix.len()..];
        let len = suffix
            .iter()
            .take(max_suffix)
            .take_while(|b| allowed(**b))
            .count();
        if len >= min_suffix {
            found.push(SecretMatch {
                kind,
                start,
                end: start + prefix.len() + len,
            });
        }
    }
}

fn find_fixed(
    text: &str,
    prefix: &str,
    suffix_len: usize,
    allowed: fn(u8) -> bool,
    kind: SecretKind,
    found: &mut Vec<SecretMatch>,
) {
    for (start, _) in text.match_indices(prefix) {
        if start > 0 && allowed(text.as_bytes()[start - 1]) {
            continue;
        }
        let suffix_start = start + prefix.len();
        let bytes = text.as_bytes();
        let end = suffix_start + suffix_len;
        if end > bytes.len() || !bytes[suffix_start..end].iter().all(|b| allowed(*b)) {
            continue;
        }
        if bytes.get(end).is_some_and(|b| allowed(*b)) {
            continue;
        }
        found.push(SecretMatch { kind, start, end });
    }
}

fn find_private_keys(text: &str, found: &mut Vec<SecretMatch>) {
    for label in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
    ] {
        let begin = format!("-----BEGIN {label}-----");
        let end_marker = format!("-----END {label}-----");
        for (start, _) in text.match_indices(&begin) {
            let after_header = start + begin.len();
            let end = text[after_header..]
                .find(&end_marker)
                .map(|offset| after_header + offset + end_marker.len())
                .unwrap_or(after_header);
            found.push(SecretMatch {
                kind: SecretKind::PrivateKey,
                start,
                end,
            });
        }
    }
}

fn find_secret_assignments(text: &str, found: &mut Vec<SecretMatch>) {
    let bytes = text.as_bytes();
    let mut cursor = 0;
    while cursor < bytes.len() {
        if !(bytes[cursor].is_ascii_alphabetic() || bytes[cursor] == b'_') {
            cursor += 1;
            continue;
        }

        let name_start = cursor;
        cursor += 1;
        while cursor < bytes.len()
            && (bytes[cursor].is_ascii_alphanumeric() || bytes[cursor] == b'_')
        {
            cursor += 1;
        }
        let name = &text[name_start..cursor];
        if !secretish_name(name) {
            continue;
        }

        let mut value_start = cursor;
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        if bytes.get(value_start) != Some(&b'=') {
            continue;
        }
        value_start += 1;
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        if matches!(bytes.get(value_start), Some(b'\'') | Some(b'"')) {
            value_start += 1;
        }

        let mut end = value_start;
        while let Some(byte) = bytes.get(end) {
            if byte.is_ascii_whitespace()
                || matches!(byte, b'\'' | b'"' | b',' | b';' | b'\\' | b'}' | b']')
            {
                break;
            }
            end += 1;
        }
        let value = &text[value_start..end];
        if value.len() >= 12 && !looks_like_placeholder(value) {
            found.push(SecretMatch {
                kind: SecretKind::EnvironmentSecret,
                start: value_start,
                end,
            });
        }
        cursor = end.max(cursor);
    }
}

fn secretish_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    upper == "TOKEN"
        || upper == "PASSWORD"
        || upper == "PRIVATE_KEY"
        || upper.ends_with("_TOKEN")
        || upper.ends_with("_PASSWORD")
        || upper.ends_with("_API_KEY")
        || upper.ends_with("_PRIVATE_KEY")
        || upper.contains("_SECRET")
        || upper.contains("SECRET_ACCESS_KEY")
}

fn looks_like_placeholder(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with('<')
        || lower.starts_with('$')
        || lower.starts_with("example")
        || lower.starts_with("placeholder")
        || lower.starts_with("changeme")
        || lower.starts_with("replace_me")
        || lower.starts_with("your_")
}

fn token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn bearer_char(byte: u8) -> bool {
    token_char(byte) || matches!(byte, b'.' | b'~' | b'+' | b'/' | b'=')
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::ports::{PortResult, RawEventSource};
    use ct_domain::{
        AgentKind, Event, EventId, EventKind, FileId, MessageRole, SessionId, SessionMetadata,
        TokenUsage, Turn,
    };

    #[test]
    fn recognises_curated_provider_tokens_without_retaining_values() {
        let text = concat!(
            "openai=sk-proj-abcdefghijklmnopqrstuvwxyz012345 ",
            "anthropic=sk-ant-api03-abcdefghijklmnopqrstuvwxyz012345 ",
            "github=ghp_abcdefghijklmnopqrstuvwxyz0123456789AB ",
            "aws=AKIAIOSFODNN7EXAMPLE"
        );
        let matches = find_secrets(text);
        let kinds: Vec<_> = matches.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                SecretKind::OpenAiApiKey,
                SecretKind::AnthropicApiKey,
                SecretKind::GitHubToken,
                SecretKind::AwsAccessKey,
            ]
        );
    }

    #[test]
    fn short_examples_are_not_reported_as_credentials() {
        assert!(find_secrets("sk-example ghp_example AKIAEXAMPLE Bearer token").is_empty());
    }

    #[test]
    fn finds_secret_like_env_assignments_but_skips_placeholders() {
        let matches = find_secrets(
            "AWS_SECRET_ACCESS_KEY='wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY'\n\
             DATABASE_PASSWORD=your_password_here\n\
             API_KEY=short",
        );
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, SecretKind::EnvironmentSecret);
    }

    #[test]
    fn redaction_preserves_context_and_names_only_the_kind() {
        let value = "run --token ghp_abcdefghijklmnopqrstuvwxyz0123456789AB now";
        let (redacted, count) = redact_text(value);
        assert_eq!(count, 1);
        assert_eq!(redacted, "run --token [REDACTED:github-token] now");
        assert!(!redacted.contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn a_private_key_is_redacted_as_one_value_including_its_body() {
        let pem =
            "before -----BEGIN PRIVATE KEY-----\nsecret-body\n-----END PRIVATE KEY----- after";
        let (redacted, count) = redact_text(pem);
        assert_eq!(count, 1);
        assert_eq!(redacted, "before [REDACTED:private-key] after");
    }

    struct Lines(&'static str);

    impl RawEventSource for Lines {
        fn fetch(&self, _source: SourceRef) -> PortResult<String> {
            Ok(self.0.to_string())
        }
    }

    #[test]
    fn reports_only_kind_and_location_for_context_records() {
        let source = SourceRef::new(FileId(0), 0, 100, 7);
        let event = Event {
            id: EventId::Ordinal(7),
            sequence: 6,
            timestamp: None,
            kind: EventKind::Message {
                role: MessageRole::User,
                preview: String::new(),
                char_len: 50,
            },
            source,
            raw_type: "user".into(),
            turn: Some(TurnNumber::new(2).unwrap()),
            links: Default::default(),
            content_measurement: None,
        };
        let session = AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            vec![event],
            vec![Turn {
                number: TurnNumber::new(2).unwrap(),
                timestamp: None,
                model: None,
                usage: TokenUsage::default(),
                event_indices: vec![0],
                anchor_index: Some(0),
            }],
            vec![],
        );
        let report = ContextTrace::new(vec![]).scan_secrets(
            &session,
            &Lines("token=sk-proj-abcdefghijklmnopqrstuvwxyz012345"),
        );

        assert_eq!(report.occurrence_count(), 1);
        assert_eq!(report.findings[0].kind, SecretKind::OpenAiApiKey);
        assert_eq!(report.findings[0].line_no, 7);
        assert_eq!(report.findings[0].turn, Some(TurnNumber::new(2).unwrap()));
    }
}
