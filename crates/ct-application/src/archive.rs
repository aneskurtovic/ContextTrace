//! Archive use cases and the two record transforms ingestion is offered.
//!
//! [`RedactingTransform`] is the safe default: recognised credential shapes are
//! replaced on the way into the archive, using the same scanner
//! [`crate::secrets`] runs for read-only inspection. [`VerbatimTransform`] is
//! the explicit opt-out -- a caller who wants byte-for-byte retention has to
//! name a type that says so.
//!
//! Both operate on the exact raw JSONL line an [`ArchiveStore`] streams in
//! ([`RecordTransform::apply`]'s only argument), which is also what
//! [`crate::secrets::scan_secrets`] scans -- the two are looking at identical
//! bytes, so a value invisible to one is invisible to the other.

use crate::secrets::redact_text;
use crate::{AppError, ContextTrace};
use ct_domain::model::archive::{ArchiveEntry, ArchiveIntegrity, RedactionMode};
use ct_domain::ports::{ArchiveStore, RecordTransform};
use std::borrow::Cow;

/// Replace recognised credential shapes on the way into the archive. The
/// default -- see [`default_transform`].
///
/// Uses the same scanner [`crate::secrets::scan_secrets`] runs, so a value
/// invisible to one is invisible to the other. It applies it **inside each JSON
/// string of the record** rather than to the record as one blob, for the reason
/// [`redact_json_record`] gives -- the two callers of [`redact_text`] do not
/// mean the same thing by "record", and running it over a whole JSONL line can
/// eat the line's own punctuation.
#[derive(Debug, Clone, Copy, Default)]
pub struct RedactingTransform;

impl RecordTransform for RedactingTransform {
    fn apply<'a>(&self, record: &'a str) -> (Cow<'a, str>, u32) {
        redact_json_record(record)
    }

    fn mode(&self) -> RedactionMode {
        RedactionMode::Redacted
    }
}

/// Redact within a raw JSONL record's string values, leaving its structure
/// untouched.
///
/// # Why this is not simply `redact_text(record)`
///
/// [`redact_text`] was written for `ct export --redact-secrets`, which hands it
/// one **already-parsed field value**. For that caller, `find_private_keys`
/// claiming everything to the end of its input when a PEM block never closes is
/// exactly right: truncated tool output is the material this project reads most,
/// and leaving the tail of a key unredacted would be worse than over-redacting
/// (CT-049).
///
/// Handing it a whole JSONL line changes what "the end of its input" means. The
/// line ends with the record's own closing `"` and `}`, so an unterminated key
/// block swallows them and the redacted line is no longer JSON at all. The
/// scanner is not wrong; the two callers simply mean different things by
/// "record", and the archive is the one that has punctuation to protect.
///
/// So each string's contents are redacted separately. Everything outside a
/// string -- key order, whitespace, escapes, structure -- is copied byte for
/// byte, which is what keeps an archive of a session holding no credentials
/// byte-identical to its log. An unterminated key block now claims the rest of
/// *its own string* and stops at the closing quote, which is the same rule doing
/// the same job at the right boundary.
///
/// A record that is not well-formed enough to walk -- not JSON at all, or a line
/// truncated mid-string -- falls back to redacting the whole thing. There is no
/// structure left to protect in that case, and failing to redact would be the
/// worse of the two mistakes.
fn redact_json_record(record: &str) -> (Cow<'_, str>, u32) {
    let Some(spans) = json_string_spans(record) else {
        let (text, count) = redact_text(record);
        return (text, count as u32);
    };
    if spans.is_empty() {
        let (text, count) = redact_text(record);
        return (text, count as u32);
    }

    let mut out: Option<String> = None;
    let mut cursor = 0usize;
    let mut total = 0u32;
    for (start, end) in spans {
        let (redacted, count) = redact_text(&record[start..end]);
        if count == 0 {
            continue;
        }
        let buffer = out.get_or_insert_with(|| String::with_capacity(record.len()));
        buffer.push_str(&record[cursor..start]);
        buffer.push_str(&redacted);
        cursor = end;
        total += count as u32;
    }

    match out {
        // Nothing matched, so the caller may keep the original bytes. The store
        // reads this borrow as "byte-identical to the log".
        None => (Cow::Borrowed(record), 0),
        Some(mut buffer) => {
            buffer.push_str(&record[cursor..]);
            (Cow::Owned(buffer), total)
        }
    }
}

/// Byte ranges of each JSON string's *contents* in a raw record.
///
/// `None` when the record ends inside a string, which means it is truncated or
/// not JSON -- the caller then has nothing to protect and redacts the lot.
///
/// Deliberately a quote-and-escape walk rather than a parse. Parsing would mean
/// re-serialising to get the record back, and re-serialisation normalises key
/// order, whitespace and escapes -- so every archived record would differ from
/// its log line, and the byte-identity that makes an unredacted archive an exact
/// copy would be lost for the sake of records that needed nothing done to them.
fn json_string_spans(record: &str) -> Option<Vec<(usize, usize)>> {
    let bytes = record.as_bytes();
    let mut spans = Vec::new();
    let mut open: Option<usize> = None;
    let mut index = 0usize;

    while index < bytes.len() {
        match bytes[index] {
            // Inside a string, a backslash escapes whatever follows -- including
            // a quote, which must not be read as the string's end. Skipping the
            // escaped byte cannot land mid-character: UTF-8 continuation bytes
            // are all >= 0x80 and so never equal a quote or a backslash.
            b'\\' if open.is_some() => index += 2,
            b'"' => {
                match open {
                    Some(start) => {
                        spans.push((start, index));
                        open = None;
                    }
                    // Contents start after the opening quote. Both ends are ASCII
                    // positions, so every slice taken from them is on a character
                    // boundary.
                    None => open = Some(index + 1),
                }
                index += 1;
            }
            _ => index += 1,
        }
    }

    open.is_none().then_some(spans)
}

/// Copy records byte for byte, credentials included.
///
/// Never allocates: every record is returned as `Cow::Borrowed`, whatever it
/// contains. Only ever reached by an explicit request for raw retention --
/// there is no path that constructs this as a fallback. Deliberately does not
/// derive `Default`: the path of least resistance (calling `Default::default`
/// on some inferred transform type) must not be able to land here.
#[derive(Debug, Clone, Copy)]
pub struct VerbatimTransform;

impl RecordTransform for VerbatimTransform {
    fn apply<'a>(&self, record: &'a str) -> (Cow<'a, str>, u32) {
        (Cow::Borrowed(record), 0)
    }

    fn mode(&self) -> RedactionMode {
        RedactionMode::Raw
    }
}

/// The transform reached for when a caller does not choose one explicitly.
///
/// A named function rather than leaving callers to pick a struct by
/// convention: this is the one symbol whose result the "default is
/// redaction" property is about, and it exists so that property has somewhere
/// concrete to be pinned by a test.
pub fn default_transform() -> RedactingTransform {
    RedactingTransform
}

impl ContextTrace {
    /// Copy one session's records into `store`, transforming each per `mode`.
    ///
    /// Resolves `id` the same way [`ContextTrace::resolve`] does for every
    /// other use case, and deliberately stops there: archiving copies the
    /// session's raw lines and never parses them into an [`AgentSession`],
    /// so the descriptor [`ContextTrace::resolve`] already produces is
    /// everything this needs. Reaching for [`ContextTrace::load`] here would
    /// pay for a full parse this use case has no reason to want.
    ///
    /// [`AgentSession`]: ct_domain::AgentSession
    pub fn archive_session(
        &self,
        id: &str,
        store: &dyn ArchiveStore,
        mode: RedactionMode,
    ) -> Result<ArchiveEntry, AppError> {
        let resolved = self.resolve(id)?;
        let entry = match mode {
            RedactionMode::Redacted => store.ingest(&resolved.descriptor, &RedactingTransform)?,
            RedactionMode::Raw => store.ingest(&resolved.descriptor, &VerbatimTransform)?,
        };
        Ok(entry)
    }

    /// Every session held in `store`, most recently archived first.
    ///
    /// The port promises no order ([`ArchiveStore::entries`]), so ordering
    /// them usefully is this use case's job, not the store's.
    pub fn archived_sessions(
        &self,
        store: &dyn ArchiveStore,
    ) -> Result<Vec<ArchiveEntry>, AppError> {
        let mut entries = store.entries()?;
        // `entries()` promises no order, so ties on `archived_at` need a
        // deterministic tiebreaker too -- otherwise "stable" would hold only
        // by accident of whatever order the store happened to iterate in.
        entries.sort_by(|a, b| {
            b.archived_at
                .cmp(&a.archived_at)
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        Ok(entries)
    }

    /// Re-digest one archived session by id or unambiguous id prefix.
    ///
    /// [`ArchiveStore::verify`] takes an agent and an id, but a source log may
    /// already be gone by the time this is called -- the exact case this
    /// feature exists to serve -- so resolving through the live corpus the
    /// way [`ContextTrace::resolve`] does is not an option here. This instead
    /// resolves against what the archive itself holds, with the same
    /// exact-match-wins-then-unambiguous-prefix rule so an archived session's
    /// id behaves the same way at this call site as everywhere else.
    pub fn verify_archived(
        &self,
        id: &str,
        store: &dyn ArchiveStore,
    ) -> Result<ArchiveIntegrity, AppError> {
        let entries = store.entries()?;

        let mut prefix_matches: Vec<&ArchiveEntry> = Vec::new();
        for entry in &entries {
            if entry.id().as_str() == id {
                return Ok(store.verify(entry.agent(), entry.id().as_str())?);
            }
            if entry.id().matches_prefix(id) {
                prefix_matches.push(entry);
            }
        }

        match prefix_matches.len() {
            0 => Err(AppError::SessionNotFound(id.to_string())),
            1 => {
                let entry = prefix_matches[0];
                Ok(store.verify(entry.agent(), entry.id().as_str())?)
            }
            _ => Err(AppError::AmbiguousSession {
                prefix: id.to_string(),
                matches: prefix_matches
                    .iter()
                    .take(5)
                    .map(|m| m.id().to_string())
                    .collect(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentBinding, ContextTrace};
    use chrono::{DateTime, Utc};
    use ct_domain::ports::{
        AgentAdapter, PortError, PortResult, ReconstructedContext, TokenEstimator,
    };
    use ct_domain::{
        AgentKind, AgentSession, SessionDescriptor, SessionId, ThreadRole, TurnNumber,
    };
    use std::collections::HashMap;
    use std::sync::Mutex;

    // AWS's own documentation example access key id. Not a real credential;
    // safe to embed in tests and fixtures. Every "credential" below reuses
    // this one value rather than inventing a new shape.
    const FAKE_AWS_KEY: &str = "AKIAIOSFODNN7EXAMPLE";

    // ---- RecordTransform behaviour -----------------------------------

    #[test]
    fn redacting_a_clean_record_borrows_and_replaces_nothing() {
        let record = r#"{"type":"user","message":{"content":"just an ordinary line"}}"#;
        let (text, count) = RedactingTransform.apply(record);
        assert_eq!(count, 0);
        assert!(
            matches!(text, Cow::Borrowed(_)),
            "a clean record must not be reallocated: it is what makes most \
             archives byte-identical copies"
        );
        assert_eq!(text, record);
    }

    #[test]
    fn redacting_a_record_carrying_a_credential_replaces_it_and_counts_it() {
        let record = format!(r#"{{"content":"aws key: {FAKE_AWS_KEY} in the output"}}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(
            matches!(text, Cow::Owned(_)),
            "a record actually changed must not claim to be borrowed"
        );
        assert!(
            !text.contains(FAKE_AWS_KEY),
            "no part of the original credential may survive: {text}"
        );
        assert!(text.contains("[REDACTED:aws-access-key-id]"));
    }

    #[test]
    fn verbatim_never_allocates_even_over_a_credential() {
        let record = format!(r#"{{"content":"{FAKE_AWS_KEY}"}}"#);
        let (text, count) = VerbatimTransform.apply(&record);
        assert_eq!(count, 0);
        assert!(matches!(text, Cow::Borrowed(_)));
        assert_eq!(text, record);
        assert_eq!(VerbatimTransform.mode(), RedactionMode::Raw);
    }

    #[test]
    fn the_transform_reached_for_by_default_redacts_rather_than_preserving_raw_credentials() {
        // Pinned separately from the two tests above: this is the property
        // that would break if a later change quietly pointed
        // `default_transform` at `VerbatimTransform` instead.
        let transform = default_transform();
        assert_eq!(transform.mode(), RedactionMode::Redacted);

        let record = format!(r#"{{"content":"{FAKE_AWS_KEY}"}}"#);
        let (text, count) = transform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
    }

    // ---- redaction must keep a JSONL record parseable -----------------

    fn assert_still_valid_json(label: &str, text: &str) {
        serde_json::from_str::<serde_json::Value>(text)
            .unwrap_or_else(|e| panic!("{label}: redaction produced invalid JSON: {e}\n{text}"));
    }

    #[test]
    fn a_credential_inside_a_plain_json_string_survives_redaction_as_valid_json() {
        let record = format!(r#"{{"type":"tool_result","content":"key={FAKE_AWS_KEY} done"}}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
        assert_still_valid_json("plain string", &text);
    }

    #[test]
    fn a_credential_next_to_escape_sequences_in_the_same_string_survives_redaction() {
        // The same JSON string value also carries an escaped quote, an
        // escaped backslash, an escaped newline and a \uXXXX escape, all
        // sitting right next to the credential. This is a raw string, so
        // `\u00e9` reaches the JSON text as the six literal characters that
        // make up that escape, not as a pre-decoded character -- exactly what
        // a raw JSONL line on disk would contain.
        let record =
            format!(r#"{{"content":"before \"quoted\" \\ \n \u00e9 {FAKE_AWS_KEY} after"}}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1, "text was: {text}");
        assert!(!text.contains(FAKE_AWS_KEY));
        assert_still_valid_json("escape sequences", &text);

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let content = parsed["content"].as_str().unwrap();
        assert!(content.contains("[REDACTED:aws-access-key-id]"));
        assert!(content.contains("quoted"));
    }

    #[test]
    fn a_credential_inside_a_nested_object_survives_redaction_as_valid_json() {
        let record = format!(r#"{{"outer":{{"inner":{{"secret":"{FAKE_AWS_KEY}"}}}}}}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
        assert_still_valid_json("nested object", &text);

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            parsed["outer"]["inner"]["secret"],
            "[REDACTED:aws-access-key-id]"
        );
    }

    #[test]
    fn a_credential_inside_a_json_array_survives_redaction_as_valid_json() {
        let record = format!(r#"{{"items":["{FAKE_AWS_KEY}","other","third"]}}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
        assert_still_valid_json("array", &text);

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["items"][0], "[REDACTED:aws-access-key-id]");
        assert_eq!(parsed["items"][1], "other");
    }

    #[test]
    fn a_record_holding_an_unterminated_key_block_still_parses_after_redaction() {
        // The case that forced `redact_json_record` to exist. `find_private_keys`
        // claims everything to the end of its input when a PEM block never
        // closes (CT-049), which is right for `ct export`, where its input is
        // one already-parsed field. Handed a whole JSONL line it used to eat the
        // record's own closing `"` and `}`, leaving something that was not JSON
        // -- an archived record that no parser could read, which is worse than
        // no archive at all.
        //
        // Redacting inside each string instead puts the same rule at the right
        // boundary: the match still claims the rest of the key, and stops at the
        // quote that ends the string holding it.
        let record = concat!(
            r#"{"type":"tool_result","content":"-----BEGIN PRIVATE KEY-----\n"#,
            r#"MIIEow[truncated"}"#
        );
        let (text, count) = RedactingTransform.apply(record);
        assert_eq!(count, 1, "text was: {text}");
        assert!(!text.contains("MIIEow"));
        assert_still_valid_json("unterminated key block", &text);
    }

    #[test]
    fn everything_outside_a_string_is_copied_byte_for_byte() {
        // The property that keeps an archive an exact copy. Redaction rewrites
        // string contents and nothing else, so key order, spacing and escapes
        // survive -- a re-serialising implementation would normalise all three
        // and make every archived record differ from its log line.
        let record =
            format!(r#"{{ "zeta":1 ,  "key" : "{FAKE_AWS_KEY}", "note":"a\tbé", "n":[1, 2] }}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert_eq!(
            text,
            r#"{ "zeta":1 ,  "key" : "[REDACTED:aws-access-key-id]", "note":"a\tbé", "n":[1, 2] }"#
        );
    }

    #[test]
    fn a_quote_inside_a_string_does_not_end_it() {
        // An escaped quote read as a terminator would split one string into two
        // spans and shift every span after it, so a credential could be scanned
        // as though it sat outside any string at all.
        let record = format!(r#"{{"said":"he said \"{FAKE_AWS_KEY}\" once"}}"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
        assert_still_valid_json("escaped quote", &text);
    }

    #[test]
    fn a_record_that_is_not_json_is_still_redacted() {
        // No structure to protect, so the whole-record rule applies and nothing
        // is lost by it. Failing to redact would be the worse of the two
        // mistakes: the archive is the one place this tool writes.
        let record = format!("plain text carrying {FAKE_AWS_KEY} and no JSON at all");
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
    }

    #[test]
    fn a_record_truncated_mid_string_is_still_redacted() {
        // A line cut off mid-write never closes its string, so it cannot be
        // walked. It has no closing punctuation left to protect either.
        let record = format!(r#"{{"content":"{FAKE_AWS_KEY} and then the file end"#);
        let (text, count) = RedactingTransform.apply(&record);
        assert_eq!(count, 1);
        assert!(!text.contains(FAKE_AWS_KEY));
    }

    #[test]
    fn a_clean_record_is_returned_borrowed_whatever_its_shape() {
        // The borrow is how `ArchiveStore::ingest` recognises a byte-identical
        // copy, so it has to survive the string walk, not just the early exit.
        for record in [
            r#"{"a":"nothing here","b":[1,2],"c":{"d":"plain"}}"#,
            "not json at all",
            r#"{"truncated":"mid string"#,
        ] {
            let (text, count) = RedactingTransform.apply(record);
            assert_eq!(count, 0, "record: {record}");
            assert!(matches!(text, Cow::Borrowed(_)), "record: {record}");
        }
    }

    // ---- fakes for the use-case tests ----------------------------------

    /// An in-memory `ArchiveStore`. Real records are supplied per session id
    /// so `ingest` has something to run the transform over, the way a real
    /// store would run it over lines read from disk.
    #[derive(Default)]
    struct FakeArchiveStore {
        sources: HashMap<String, Vec<String>>,
        entries: Mutex<HashMap<(AgentKind, String), ArchiveEntry>>,
    }

    impl FakeArchiveStore {
        fn with_source(id: &str, lines: Vec<&str>) -> Self {
            let mut sources = HashMap::new();
            sources.insert(
                id.to_string(),
                lines.into_iter().map(String::from).collect(),
            );
            Self {
                sources,
                entries: Mutex::new(HashMap::new()),
            }
        }

        fn seeded_entry(id: &str, agent: AgentKind, archived_at: DateTime<Utc>) -> ArchiveEntry {
            ArchiveEntry {
                descriptor: descriptor(id, agent),
                archived_at,
                redaction: RedactionMode::Redacted,
                records: 1,
                source_bytes: 10,
                source_digest: "src".into(),
                archived_bytes: 10,
                archived_digest: "arc".into(),
                redacted_records: 0,
                redacted_values: 0,
            }
        }

        fn seed(&self, entry: ArchiveEntry) {
            self.entries
                .lock()
                .unwrap()
                .insert((entry.agent(), entry.id().to_string()), entry);
        }
    }

    impl ArchiveStore for FakeArchiveStore {
        fn root(&self) -> String {
            "fake://archive".into()
        }

        fn ingest(
            &self,
            descriptor: &SessionDescriptor,
            transform: &dyn RecordTransform,
        ) -> PortResult<ArchiveEntry> {
            let lines = self
                .sources
                .get(descriptor.id.as_str())
                .cloned()
                .unwrap_or_default();

            let mut source_bytes = 0u64;
            let mut archived_bytes = 0u64;
            let mut redacted_records = 0u64;
            let mut redacted_values = 0u64;

            for line in &lines {
                source_bytes += line.len() as u64;
                let (text, count) = transform.apply(line);
                archived_bytes += text.len() as u64;
                if count > 0 {
                    redacted_records += 1;
                    redacted_values += u64::from(count);
                }
            }

            let entry = ArchiveEntry {
                descriptor: descriptor.clone(),
                archived_at: DateTime::UNIX_EPOCH,
                redaction: transform.mode(),
                records: lines.len() as u64,
                source_bytes,
                source_digest: "src-digest".into(),
                archived_bytes,
                archived_digest: "archived-digest".into(),
                redacted_records,
                redacted_values,
            };
            self.seed(entry.clone());
            Ok(entry)
        }

        fn entries(&self) -> PortResult<Vec<ArchiveEntry>> {
            Ok(self.entries.lock().unwrap().values().cloned().collect())
        }

        fn entry(&self, agent: AgentKind, id: &str) -> PortResult<Option<ArchiveEntry>> {
            Ok(self
                .entries
                .lock()
                .unwrap()
                .get(&(agent, id.to_string()))
                .cloned())
        }

        fn verify(&self, agent: AgentKind, id: &str) -> PortResult<ArchiveIntegrity> {
            if self
                .entries
                .lock()
                .unwrap()
                .contains_key(&(agent, id.to_string()))
            {
                Ok(ArchiveIntegrity::Intact)
            } else {
                Err(PortError::NotFound(id.to_string()))
            }
        }
    }

    fn descriptor(id: &str, agent: AgentKind) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new(id).unwrap(),
            agent,
            path: format!("fake-path/{id}.jsonl"),
            size_bytes: 100,
            project: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        }
    }

    /// The adapter behind [`ContextTrace::resolve`] in these tests. It only
    /// ever needs to discover descriptors -- `archive_session` never parses.
    struct FakeAdapter {
        agent: AgentKind,
        sessions: Vec<SessionDescriptor>,
    }

    impl AgentAdapter for FakeAdapter {
        fn agent(&self) -> AgentKind {
            self.agent
        }
        fn roots(&self) -> Vec<String> {
            Vec::new()
        }
        fn discover(&self) -> PortResult<Vec<SessionDescriptor>> {
            Ok(self.sessions.clone())
        }
        fn load(&self, _descriptor: &SessionDescriptor) -> PortResult<AgentSession> {
            unimplemented!("archive_session must resolve without a full parse")
        }
        fn reconstruct(
            &self,
            _session: &AgentSession,
            _turn: TurnNumber,
            _estimator: &dyn TokenEstimator,
        ) -> PortResult<ReconstructedContext> {
            unimplemented!("not exercised by archive use cases")
        }
    }

    struct CharProbe;
    impl TokenEstimator for CharProbe {
        fn count_text(&self, text: &str) -> ct_domain::TokenCount {
            ct_domain::TokenCount::estimated(text.chars().count() as u32)
        }
        fn estimate_from_chars(&self, char_len: u32) -> ct_domain::TokenCount {
            ct_domain::TokenCount::estimated(char_len)
        }
        fn name(&self) -> &str {
            "characters"
        }
    }

    fn app_with(sessions: Vec<SessionDescriptor>) -> ContextTrace {
        ContextTrace::new(vec![AgentBinding::new(
            Box::new(FakeAdapter {
                agent: AgentKind::Codex,
                sessions,
            }),
            Box::new(CharProbe),
        )])
    }

    // ---- archive_session -------------------------------------------------

    #[test]
    fn archiving_a_session_ingests_it_through_the_requested_mode() {
        let app = app_with(vec![descriptor("abc123", AgentKind::Codex)]);
        let store =
            FakeArchiveStore::with_source("abc123", vec![r#"{"type":"user","content":"hello"}"#]);

        let entry = app
            .archive_session("abc123", &store, RedactionMode::Redacted)
            .expect("a discovered session archives");

        assert_eq!(entry.redaction, RedactionMode::Redacted);
        assert_eq!(entry.records, 1);
        assert_eq!(entry.redacted_values, 0);
        assert!(!entry.differs_from_source());
    }

    #[test]
    fn archiving_with_redacted_mode_replaces_a_credential_and_reports_it() {
        let app = app_with(vec![descriptor("withsecret", AgentKind::Codex)]);
        let line = format!(r#"{{"content":"{FAKE_AWS_KEY}"}}"#);
        let store = FakeArchiveStore::with_source("withsecret", vec![&line]);

        let entry = app
            .archive_session("withsecret", &store, RedactionMode::Redacted)
            .expect("archives");

        assert_eq!(entry.redacted_records, 1);
        assert_eq!(entry.redacted_values, 1);
        assert!(entry.differs_from_source());
        // `[REDACTED:aws-access-key-id]` (29 bytes) is longer than the
        // 20-byte key it replaces, so the archived copy is not merely
        // non-empty -- it is measurably larger than the source it came from.
        assert!(entry.archived_bytes > entry.source_bytes);
    }

    #[test]
    fn archiving_with_raw_mode_retains_a_credential_untouched() {
        let app = app_with(vec![descriptor("rawsecret", AgentKind::Codex)]);
        let line = format!(r#"{{"content":"{FAKE_AWS_KEY}"}}"#);
        let store = FakeArchiveStore::with_source("rawsecret", vec![&line]);

        let entry = app
            .archive_session("rawsecret", &store, RedactionMode::Raw)
            .expect("archives");

        assert_eq!(entry.redaction, RedactionMode::Raw);
        assert_eq!(entry.redacted_values, 0);
        assert_eq!(entry.archived_bytes, entry.source_bytes);
        assert!(!entry.differs_from_source());
    }

    #[test]
    fn archiving_an_id_that_resolves_to_nothing_is_an_error() {
        let app = app_with(vec![]);
        let store = FakeArchiveStore::default();
        let err = app
            .archive_session("nope", &store, RedactionMode::Redacted)
            .expect_err("no such session");
        assert!(matches!(err, AppError::SessionNotFound(id) if id == "nope"));
    }

    // ---- archived_sessions -------------------------------------------------

    #[test]
    fn archived_sessions_are_listed_most_recently_archived_first() {
        let store = FakeArchiveStore::default();
        store.seed(FakeArchiveStore::seeded_entry(
            "older",
            AgentKind::Codex,
            DateTime::UNIX_EPOCH,
        ));
        store.seed(FakeArchiveStore::seeded_entry(
            "newer",
            AgentKind::Codex,
            DateTime::UNIX_EPOCH + chrono::Duration::days(1),
        ));

        let app = app_with(vec![]);
        let entries = app.archived_sessions(&store).expect("entries list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].id().as_str(), "newer");
        assert_eq!(entries[1].id().as_str(), "older");
    }

    #[test]
    fn archived_sessions_is_empty_rather_than_an_error_when_the_archive_is_empty() {
        let app = app_with(vec![]);
        let store = FakeArchiveStore::default();
        assert!(app.archived_sessions(&store).unwrap().is_empty());
    }

    // ---- verify_archived -------------------------------------------------

    #[test]
    fn verifying_an_archived_session_by_exact_id_reports_its_integrity() {
        let store = FakeArchiveStore::default();
        store.seed(FakeArchiveStore::seeded_entry(
            "abc123",
            AgentKind::Codex,
            DateTime::UNIX_EPOCH,
        ));
        let app = app_with(vec![]);

        let integrity = app
            .verify_archived("abc123", &store)
            .expect("archived session verifies");
        assert_eq!(integrity, ArchiveIntegrity::Intact);
    }

    #[test]
    fn verifying_by_an_unambiguous_prefix_finds_the_one_archived_session() {
        let store = FakeArchiveStore::default();
        store.seed(FakeArchiveStore::seeded_entry(
            "abcdef",
            AgentKind::Codex,
            DateTime::UNIX_EPOCH,
        ));
        let app = app_with(vec![]);

        let integrity = app
            .verify_archived("abc", &store)
            .expect("prefix resolves to the one archived session");
        assert_eq!(integrity, ArchiveIntegrity::Intact);
    }

    #[test]
    fn verifying_an_id_that_resolves_to_no_archived_session_is_an_error() {
        let app = app_with(vec![]);
        let store = FakeArchiveStore::default();
        let err = app
            .verify_archived("missing", &store)
            .expect_err("nothing archived under this id");
        assert!(matches!(err, AppError::SessionNotFound(id) if id == "missing"));
    }

    #[test]
    fn verifying_an_ambiguous_prefix_across_two_archived_sessions_is_an_error() {
        let store = FakeArchiveStore::default();
        store.seed(FakeArchiveStore::seeded_entry(
            "abc111",
            AgentKind::Codex,
            DateTime::UNIX_EPOCH,
        ));
        store.seed(FakeArchiveStore::seeded_entry(
            "abc222",
            AgentKind::ClaudeCode,
            DateTime::UNIX_EPOCH,
        ));
        let app = app_with(vec![]);

        let err = app
            .verify_archived("abc", &store)
            .expect_err("two archived sessions share this prefix");
        assert!(matches!(err, AppError::AmbiguousSession { .. }));
    }
}
