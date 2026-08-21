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
    GitLabToken,
    NpmToken,
    AwsAccessKey,
    GoogleApiKey,
    SlackToken,
    StripeLiveKey,
    BearerToken,
    JsonWebToken,
    PrivateKey,
    EnvironmentSecret,
}

impl SecretKind {
    pub fn label(self) -> &'static str {
        match self {
            SecretKind::OpenAiApiKey => "OpenAI API key",
            SecretKind::AnthropicApiKey => "Anthropic API key",
            SecretKind::GitHubToken => "GitHub token",
            SecretKind::GitLabToken => "GitLab token",
            SecretKind::NpmToken => "npm token",
            SecretKind::AwsAccessKey => "AWS access key id",
            SecretKind::GoogleApiKey => "Google API key",
            SecretKind::SlackToken => "Slack token",
            SecretKind::StripeLiveKey => "Stripe live secret",
            SecretKind::BearerToken => "Bearer token",
            SecretKind::JsonWebToken => "JSON Web Token",
            SecretKind::PrivateKey => "private key",
            SecretKind::EnvironmentSecret => "secret-like assignment",
        }
    }

    pub fn marker(self) -> &'static str {
        match self {
            SecretKind::OpenAiApiKey => "openai-api-key",
            SecretKind::AnthropicApiKey => "anthropic-api-key",
            SecretKind::GitHubToken => "github-token",
            SecretKind::GitLabToken => "gitlab-token",
            SecretKind::NpmToken => "npm-token",
            SecretKind::AwsAccessKey => "aws-access-key-id",
            SecretKind::GoogleApiKey => "google-api-key",
            SecretKind::SlackToken => "slack-token",
            SecretKind::StripeLiveKey => "stripe-live-secret",
            SecretKind::BearerToken => "bearer-token",
            SecretKind::JsonWebToken => "jwt",
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
    // GitLab personal/project access tokens are `glpat-` followed by a
    // base64url-ish body; GitLab has shipped 20-character bodies since the
    // format's introduction and newer tokens append a routing suffix, so the
    // floor is the original fixed length and the ceiling just bounds the scan
    // rather than asserting a real maximum.
    find_prefixed(
        text,
        "glpat-",
        20,
        50,
        token_char,
        SecretKind::GitLabToken,
        &mut found,
    );
    // npm access tokens are `npm_` followed by exactly 36 alphanumeric
    // characters (no separators) — an npm-specific, tighter class than the
    // GitHub/GitLab tokens, which is why it gets its own predicate.
    find_fixed(
        text,
        "npm_",
        36,
        alnum_char,
        SecretKind::NpmToken,
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
    find_bare_jwts(text, &mut found);
    find_secret_assignments(text, &mut found);

    // Prefer the longest match at a shared start, then discard overlaps. This
    // keeps an Anthropic `sk-ant-...` from also becoming a generic OpenAI key,
    // a `Bearer <jwt>` match from also becoming a duplicate bare-JWT finding
    // (the bearer match starts earlier and spans the whole token, so it wins),
    // and a provider-specific `glpat-`/`npm_` match from double-reporting
    // alongside the generic secret-like-assignment detector when both land on
    // the same value.
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

/// Match a PEM block, and everything after an opening header that never closes.
///
/// A record that begins a key block and stops mid-body is the shape this
/// project reads most: bounded and truncated tool output is the material it
/// exists to analyse. Ending such a match at the header would leave the key
/// body itself in redacted output, so an unterminated block claims the rest of
/// the record. Over-redacting prose that merely quotes a `-----BEGIN` line is
/// the affordable half of that trade.
fn find_private_keys(text: &str, found: &mut Vec<SecretMatch>) {
    for label in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "EC PRIVATE KEY",
        "OPENSSH PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "DSA PRIVATE KEY",
        "PGP PRIVATE KEY BLOCK",
    ] {
        let begin = format!("-----BEGIN {label}-----");
        let end_marker = format!("-----END {label}-----");
        for (start, _) in text.match_indices(&begin) {
            let after_header = start + begin.len();
            let end = text[after_header..]
                .find(&end_marker)
                .map(|offset| after_header + offset + end_marker.len())
                .unwrap_or(text.len());
            found.push(SecretMatch {
                kind: SecretKind::PrivateKey,
                start,
                end,
            });
        }
    }
}

/// Match a secret-like name bound to a value, in either syntax that appears in
/// a session.
///
/// Everything this tool reads is JSONL, so `"api_key": "…"` is the common form
/// and `NAME=value` the exception; recognising only the shell form left the
/// generic detector unable to fire on the project's own corpus. A quoted key
/// closes before its separator, so the closing quote is stepped over rather
/// than treated as the start of the value.
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
        while cursor < bytes.len() && name_char(bytes[cursor]) {
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
        if matches!(bytes.get(value_start), Some(b'\'') | Some(b'"')) {
            value_start += 1;
        }
        while bytes.get(value_start).is_some_and(u8::is_ascii_whitespace) {
            value_start += 1;
        }
        if !matches!(bytes.get(value_start), Some(b'=') | Some(b':')) {
            continue;
        }
        // `SecretKind::EnvironmentSecret` is a path, not a binding. Rust and
        // C++ spell the qualifier with the same colon JSON uses for a member,
        // so the doubled form has to be rejected explicitly -- without this,
        // every enum whose name contains SECRET reports itself, and the first
        // corpus that happens to is this crate's own source.
        if bytes[value_start] == b':' && bytes.get(value_start + 1) == Some(&b':') {
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
        if value.len() >= 12 && !looks_like_placeholder(value) && !looks_like_code(value) {
            found.push(SecretMatch {
                kind: SecretKind::EnvironmentSecret,
                start: value_start,
                end,
            });
        }
        cursor = end.max(cursor);
    }
}

/// Decide whether a name promises a credential, whatever its spelling.
///
/// Matching runs on words rather than on underscores, so `AWS_SECRET_KEY`,
/// `aws-secret-key` and `awsSecretKey` are the same name here. JSON keys are
/// usually written in the last of those three, and keying on underscores made
/// every one of them invisible. `KEY` alone is deliberately not enough: it is
/// too common a word to carry the claim on its own.
fn secretish_name(name: &str) -> bool {
    let words = name_words(name);
    let Some(last) = words.last() else {
        return false;
    };
    if words.iter().any(|word| word == "SECRET") {
        return true;
    }
    if matches!(
        last.as_str(),
        "TOKEN" | "PASSWORD" | "PASSPHRASE" | "CREDENTIAL" | "CREDENTIALS"
    ) {
        return true;
    }
    if last != "KEY" || words.len() < 2 {
        return false;
    }
    matches!(
        words[words.len() - 2].as_str(),
        "API" | "PRIVATE" | "ACCESS" | "AUTH" | "SIGNING" | "ENCRYPTION" | "SESSION"
    )
}

/// Split a name into upper-case words across `_`, `-`, `.` and case changes.
///
/// `APIKey` splits before the final word rather than after the acronym, so
/// `xApiKey`, `X-API-KEY` and `x_api_key` all reduce to `[X, API, KEY]`.
fn name_words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut words = Vec::new();
    let mut word = String::new();
    for (index, &current) in chars.iter().enumerate() {
        if matches!(current, '_' | '-' | '.') {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            continue;
        }
        let starts_word = current.is_ascii_uppercase()
            && !word.is_empty()
            && (!chars[index - 1].is_ascii_uppercase()
                || chars.get(index + 1).is_some_and(char::is_ascii_lowercase));
        if starts_word {
            words.push(std::mem::take(&mut word));
        }
        word.push(current.to_ascii_uppercase());
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
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

/// Reject a value that is source code rather than a credential.
///
/// Grepping a codebase puts lines like
/// `applicant.UserSecretEncrypted = Crypto.Encrypt(user1.Value)` into the
/// context, and on names alone the generic assignment detector cannot tell
/// that apart from `api_secret = <literal>`. The bracket is what settles it:
/// a call expression carries one and a credential does not. Keying on the
/// bracket rather than on the dot is what leaves JWT bodies, whose segments
/// are dot-separated, firing as before. The cost is a literal password that
/// happens to contain a bracket, which this no longer reports.
fn looks_like_code(value: &str) -> bool {
    value.bytes().any(|byte| matches!(byte, b'(' | b')'))
}

/// A name may be written `api_key`, `api-key` or `apiKey`; all three continue
/// the same name here.
fn name_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn token_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

fn bearer_char(byte: u8) -> bool {
    token_char(byte) || matches!(byte, b'.' | b'~' | b'+' | b'/' | b'=')
}

/// npm token bodies are strictly alphanumeric, unlike the GitHub/GitLab
/// bodies which also allow `_`/`-`, so they get their own character class.
fn alnum_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
}

/// A JWT segment is base64url: alphanumeric plus `-`/`_`, no padding.
fn jwt_seg_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_')
}

/// Minimum plausible lengths for each dot-separated JWT segment.
///
/// These are floors, not the length of any specific algorithm's output: a
/// minimal real-world header (`{"alg":"HS256","typ":"JWT"}`, base64url) is
/// about 20 characters, a minimal payload is rarely under 8, and requiring a
/// non-trivial signature excludes the `alg: none` shape (an already-insecure,
/// rarely-encountered variant) rather than accepting an empty third segment.
const JWT_MIN_HEADER: usize = 16;
const JWT_MIN_PAYLOAD: usize = 8;
const JWT_MIN_SIGNATURE: usize = 10;

/// Match a bare JSON Web Token: three dot-separated base64url segments where
/// the first begins `eyJ`.
///
/// `eyJ` is the base64url encoding of `{"`, which every JWT header starts
/// with (`{"alg":...`); requiring it literally — rather than decoding the
/// segment — is the same low-cost heuristic other secret scanners use, and it
/// is what keeps this from flagging arbitrary dotted or base64-shaped text in
/// tool output (a source-map's embedded base64 JSON blob, for instance, has
/// no internal dots at all and fails the very next check). No new dependency
/// is pulled in to actually decode and validate the base64.
fn find_bare_jwts(text: &str, found: &mut Vec<SecretMatch>) {
    let bytes = text.as_bytes();
    for (start, _) in text.match_indices("eyJ") {
        if start > 0 && jwt_seg_char(bytes[start - 1]) {
            continue;
        }
        let mut pos = start;
        let header_len = bytes[pos..]
            .iter()
            .take_while(|b| jwt_seg_char(**b))
            .count();
        if header_len < JWT_MIN_HEADER {
            continue;
        }
        pos += header_len;
        if bytes.get(pos) != Some(&b'.') {
            continue;
        }
        pos += 1;
        let payload_len = bytes[pos..]
            .iter()
            .take_while(|b| jwt_seg_char(**b))
            .count();
        if payload_len < JWT_MIN_PAYLOAD {
            continue;
        }
        pos += payload_len;
        if bytes.get(pos) != Some(&b'.') {
            continue;
        }
        pos += 1;
        let signature_len = bytes[pos..]
            .iter()
            .take_while(|b| jwt_seg_char(**b))
            .count();
        if signature_len < JWT_MIN_SIGNATURE {
            continue;
        }
        pos += signature_len;
        found.push(SecretMatch {
            kind: SecretKind::JsonWebToken,
            start,
            end: pos,
        });
    }
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
    fn skips_a_path_qualifier_that_is_not_an_assignment() {
        // This crate's own source is the corpus that proves it: every variant
        // of an enum named for secrets reported itself once.
        let matches =
            find_secrets("matched SecretKind::EnvironmentSecret and SecretKind::AnthropicApiKey");
        assert!(matches.is_empty());
    }

    #[test]
    fn skips_assignments_whose_value_is_a_call_expression() {
        // Grep output over a C# codebase, which is how this reached the
        // detector: the name promises a credential and the value is code.
        let matches =
            find_secrets("applicant.UserSecretEncrypted = Crypto.Encrypt(user1.RawValue)");
        assert!(matches.is_empty());
    }

    #[test]
    fn still_finds_a_dotted_token_that_is_not_code() {
        // The guard above keys on the bracket, not on the dot, so a JWT body
        // -- three dot-separated base64url segments -- still fires.
        let matches = find_secrets(
            "session_token=eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.dBjftJeZ4CVPmB92K27uhbUJU1p1r_wW1gFWFOEjXk",
        );
        assert!(!matches.is_empty());
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
    fn finds_secret_like_members_in_a_json_session_record() {
        // The corpus is JSONL, so this is the shape the generic detector has to
        // work on: colon separators, quoted values and camel-case keys.
        let record = concat!(
            r#"{"type":"tool_use","name":"Bash","input":{"#,
            r#""command":"curl -H 'x-api-key: 8f2b1c9d44ae5107bd33' https://api.internal/v1"},"#,
            r#""env":{"apiKey":"9c1f2b7d8e4a6053ab21","authToken":"3e5d9a1c7b2f4068dc84","#,
            r#""clientSecret":"a71f0c4e9d2b6538fa19","DATABASE_PASSWORD":"your_password_here"},"#,
            r#""usage":{"max_tokens":4096,"model":"claude-opus-4-8"}}"#
        );

        let matches = find_secrets(record);
        assert_eq!(
            matches.len(),
            4,
            "the header, both camel-case keys and the client secret are all assignments"
        );
        assert!(matches
            .iter()
            .all(|found| found.kind == SecretKind::EnvironmentSecret));

        let (redacted, count) = redact_text(record);
        assert_eq!(count, 4);
        for value in [
            "8f2b1c9d44ae5107bd33",
            "9c1f2b7d8e4a6053ab21",
            "3e5d9a1c7b2f4068dc84",
            "a71f0c4e9d2b6538fa19",
        ] {
            assert!(!redacted.contains(value), "{value} survived redaction");
        }
        assert!(
            redacted.contains(r#""apiKey":"[REDACTED:environment-secret]""#),
            "redaction keeps the record's shape: {redacted}"
        );
        assert!(
            redacted.contains(r#""max_tokens":4096"#) && redacted.contains("claude-opus-4-8"),
            "a token count and a model name are not credentials: {redacted}"
        );
        assert!(
            redacted.contains("your_password_here"),
            "a placeholder is still not reported as a secret"
        );
    }

    #[test]
    fn a_private_key_is_redacted_as_one_value_including_its_body() {
        let pem =
            "before -----BEGIN PRIVATE KEY-----\nsecret-body\n-----END PRIVATE KEY----- after";
        let (redacted, count) = redact_text(pem);
        assert_eq!(count, 1);
        assert_eq!(redacted, "before [REDACTED:private-key] after");
    }

    #[test]
    fn a_key_block_cut_off_mid_body_is_redacted_to_the_end_of_the_record() {
        // Truncated records are the material this project reads, so the block
        // that never closes must not leak the half of the key that was written.
        let truncated = concat!(
            "tool output: -----BEGIN RSA PRIVATE KEY-----\n",
            "MIIEowIBAAKCAQEAvR8kJ2mQ1sX7pL0fY6nD4wT9cH5bV2gA3rK8mN1qS4tU6xZ0\n",
            "wB7yC9dE2fG5hJ8kL1mN4pQ7rS0tU3vW6xY9zA2bC5dE8fG1h[output truncated"
        );

        let (redacted, count) = redact_text(truncated);
        assert_eq!(count, 1);
        assert_eq!(redacted, "tool output: [REDACTED:private-key]");
        assert!(!redacted.contains("MIIEowIBAAKCAQEA"));
    }

    #[test]
    fn recognises_gitlab_and_npm_tokens_but_not_short_lookalikes() {
        // Bodies of exactly the minimum length: 20 characters for GitLab,
        // and exactly 36 (npm's fixed length) for npm.
        let gitlab_body = "abcdEFGH12345678wxyz";
        assert_eq!(gitlab_body.len(), 20);
        let npm_body: String = "aB3".repeat(12);
        assert_eq!(npm_body.len(), 36);

        let text = format!("gitlab=glpat-{gitlab_body} npm=npm_{npm_body}");
        let matches = find_secrets(&text);
        let kinds: Vec<_> = matches.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![SecretKind::GitLabToken, SecretKind::NpmToken],
            "found: {matches:?}"
        );

        // A GitLab body under the 20-character floor, and an npm body one
        // short of the fixed 36-character length, must not match.
        assert!(find_secrets("glpat-tooshortbody1").is_empty());
        let npm_short = "aB3".repeat(11) + "aB"; // 35 characters
        assert_eq!(npm_short.len(), 35);
        assert!(find_secrets(&format!("npm_{npm_short}")).is_empty());
    }

    #[test]
    fn a_gitlab_token_is_redacted_by_kind() {
        let (redacted, count) = redact_text("glpat-abcdEFGH12345678wxyz");
        assert_eq!(count, 1);
        assert_eq!(redacted, "[REDACTED:gitlab-token]");
    }

    #[test]
    fn an_npm_token_is_redacted_by_kind() {
        let npm_body: String = "aB3".repeat(12);
        let text = format!("npm_{npm_body}");
        let (redacted, count) = redact_text(&text);
        assert_eq!(count, 1);
        assert_eq!(redacted, "[REDACTED:npm-token]");
    }

    #[test]
    fn a_provider_specific_token_wins_over_the_generic_assignment_detector() {
        // "npmToken"/"gitlabToken" both satisfy `secretish_name` (last word
        // TOKEN), so the generic assignment scanner and the provider-prefix
        // scanner land on the exact same span. The specific kind must win,
        // not double-report alongside `EnvironmentSecret`.
        let npm_body: String = "aB3".repeat(12);
        let gitlab_body = "abcdEFGH12345678wxyz";
        let record =
            format!(r#"{{"npmToken":"npm_{npm_body}","gitlabToken":"glpat-{gitlab_body}"}}"#);

        let matches = find_secrets(&record);
        assert_eq!(matches.len(), 2, "found: {matches:?}");
        let kinds: Vec<_> = matches.iter().map(|m| m.kind).collect();
        assert_eq!(kinds, vec![SecretKind::NpmToken, SecretKind::GitLabToken]);
        assert!(
            !kinds.contains(&SecretKind::EnvironmentSecret),
            "the specific kind must suppress the generic one on an identical span"
        );
    }

    #[test]
    fn recognises_new_pem_labels_including_the_pgp_block() {
        for (label, body) in [
            ("ENCRYPTED PRIVATE KEY", "encrypted-body"),
            ("DSA PRIVATE KEY", "dsa-body"),
            ("PGP PRIVATE KEY BLOCK", "pgp-body"),
        ] {
            let pem =
                format!("before -----BEGIN {label}-----\n{body}\n-----END {label}----- after");
            let (redacted, count) = redact_text(&pem);
            assert_eq!(count, 1, "label {label} did not match: {pem}");
            assert_eq!(redacted, "before [REDACTED:private-key] after");
        }
    }

    #[test]
    fn a_pgp_block_cut_off_mid_body_still_redacts_to_the_end_of_the_record() {
        // CT-049's unterminated-block behaviour must hold for every label,
        // including the newly added PGP one.
        let truncated = concat!(
            "tool output: -----BEGIN PGP PRIVATE KEY BLOCK-----\n",
            "lQOYBFtest0BCAC7base64looking0content0that0never0closes[truncated"
        );
        let (redacted, count) = redact_text(truncated);
        assert_eq!(count, 1);
        assert_eq!(redacted, "tool output: [REDACTED:private-key]");
    }

    #[test]
    fn recognises_a_bare_jwt_with_no_bearer_prefix() {
        // The jwt.io debugger's own example token: a real three-segment JWT.
        let jwt = concat!(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
            "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.",
            "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        let text = format!("token seen in output: {jwt} end");
        let matches = find_secrets(&text);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].kind, SecretKind::JsonWebToken);

        let (redacted, count) = redact_text(&text);
        assert_eq!(count, 1);
        assert_eq!(redacted, "token seen in output: [REDACTED:jwt] end");
    }

    #[test]
    fn a_bare_jwt_does_not_double_report_after_a_bearer_prefix() {
        let jwt = concat!(
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.",
            "eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.",
            "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c"
        );
        let text = format!("Authorization: Bearer {jwt}");
        let matches = find_secrets(&text);
        assert_eq!(
            matches.len(),
            1,
            "the bearer-token match must win over the overlapping bare-JWT match: {matches:?}"
        );
        assert_eq!(matches[0].kind, SecretKind::BearerToken);
    }

    #[test]
    fn a_too_short_eyj_prefixed_header_does_not_match_even_with_three_segments() {
        // Exercises the JWT_MIN_HEADER floor directly: a full three-segment,
        // dot-separated, `eyJ`-prefixed shape whose header segment is still
        // too short to be a real header.
        assert!(
            find_secrets("eyJhbGc.payloadsegmentlongenough.signaturesegmentlongenough").is_empty()
        );
    }

    #[test]
    fn a_two_segment_jwt_lookalike_does_not_match() {
        // Missing the third (signature) segment entirely.
        assert!(
            find_secrets("eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.onlyonepayloadsegmenthere")
                .is_empty()
        );
    }

    #[test]
    fn a_sourcemap_style_base64_blob_is_not_mistaken_for_a_jwt() {
        // Realistic tool/build output: an inline source map data URI. The
        // embedded base64 JSON payload happens to start with the JWT header
        // signature `eyJ`, but it is one continuous run with no internal
        // dots, so it must not be reported as a bare JWT.
        let line = "//# sourceMappingURL=data:application/json;base64,\
eyJ2ZXJzaW9uIjozLCJmaWxlIjoiYnVuZGxlLmpzIiwic291cmNlcyI6W119Cg==";
        assert!(find_secrets(line).is_empty());
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
