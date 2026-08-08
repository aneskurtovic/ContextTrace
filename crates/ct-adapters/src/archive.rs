//! Filesystem implementation of [`ArchiveStore`].
//!
//! Layout, chosen so a crash costs at most the record or manifest line being
//! written when it happens:
//!
//! ```text
//! <root>/manifest.ndjson              one ArchiveEntry as JSON per line, appended
//! <root>/sessions/<agent>/<id>.jsonl  the copied records
//! ```
//!
//! The manifest is append-only and last-wins: re-ingesting a session appends a
//! new line rather than rewriting the file, so [`FileArchiveStore::entries`]
//! keeps the last entry per `(agent, id)` and silently skips anything it
//! cannot parse -- a trailing partial line from a crash mid-append included.
//! The archived copy itself is written to a temporary file in its destination
//! directory and renamed into place, so an interrupted ingest cannot leave a
//! half-written copy where a complete one used to be.

use crate::fingerprint::{hex, Sha256};
use crate::home_dir;
use chrono::{DateTime, Utc};
use ct_domain::model::archive::{ArchiveEntry, ArchiveIntegrity};
use ct_domain::ports::{ArchiveStore, PortError, PortResult, RecordTransform};
use ct_domain::{AgentKind, SessionDescriptor};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Copies sessions into a local, append-only archive.
pub struct FileArchiveStore {
    root: PathBuf,
}

impl FileArchiveStore {
    /// Resolve the default archive root: `CONTEXTTRACE_ARCHIVE` if set, else a
    /// per-user data directory. Mirrors the `CODEX_HOME` idiom in
    /// [`crate::codex`]: an explicit override first, then the conventional
    /// platform location.
    pub fn new() -> Self {
        Self::at(default_root())
    }

    /// Point the store at an explicit directory. What tests and CI use, so
    /// they never depend on -- or pollute -- whatever a real machine has under
    /// its per-user data directory.
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.ndjson")
    }

    fn sessions_dir(&self, agent: AgentKind) -> PathBuf {
        self.root.join("sessions").join(agent.label())
    }

    fn session_path(&self, agent: AgentKind, id: &str) -> PortResult<PathBuf> {
        let encoded = safe_filename(id)?;
        Ok(self.sessions_dir(agent).join(format!("{encoded}.jsonl")))
    }

    fn append_to_manifest(&self, entry: &ArchiveEntry) -> PortResult<()> {
        fs::create_dir_all(&self.root)
            .map_err(|e| PortError::Io(format!("{}: {e}", self.root.display())))?;
        let manifest_path = self.manifest_path();
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&manifest_path)
            .map_err(|e| PortError::Io(format!("{}: {e}", manifest_path.display())))?;

        let mut line = serde_json::to_string(entry)
            .map_err(|e| PortError::Io(format!("serialising archive entry: {e}")))?;
        line.push('\n');
        file.write_all(line.as_bytes())
            .map_err(|e| PortError::Io(format!("{}: {e}", manifest_path.display())))
    }
}

impl Default for FileArchiveStore {
    fn default() -> Self {
        Self::new()
    }
}

/// `%LOCALAPPDATA%\ContextTrace\archive` on Windows, `$XDG_DATA_HOME` or
/// `~/.local/share` elsewhere. Checked in this order without `cfg(windows)`
/// gating, matching [`crate::home_dir`]'s style: `LOCALAPPDATA` is simply
/// unset on platforms where it does not apply, so the fallbacks are reached
/// there without needing a compile-time split.
fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("CONTEXTTRACE_ARCHIVE") {
        return PathBuf::from(dir);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join("ContextTrace").join("archive");
    }
    if let Some(xdg) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(xdg).join("contexttrace").join("archive");
    }
    if let Some(home) = home_dir() {
        return home
            .join(".local")
            .join("share")
            .join("contexttrace")
            .join("archive");
    }
    // No environment gave us anything to anchor to. Relative to the process's
    // working directory is a worse default than any of the above, but it is
    // still a place, which is what every other branch here promises to return.
    PathBuf::from("contexttrace-archive")
}

/// Turn a session id into a filesystem-safe path component.
///
/// A session id becomes a filename, so it must not be able to smuggle a path
/// separator or a `..` component into that position -- a store that let an id
/// escape its root would be a path traversal in a tool users hand their whole
/// log directory to. `.` and `..` are rejected outright (they are the only
/// strings entirely composed of otherwise-safe characters that still name
/// something other than themselves on a filesystem); every other disallowed
/// byte is percent-encoded rather than rejected, so an odd but real id from a
/// future agent still gets archived instead of refused.
fn safe_filename(id: &str) -> PortResult<String> {
    if id.is_empty() || id == "." || id == ".." {
        return Err(PortError::Malformed {
            path: id.to_string(),
            detail: "session id is not safe to use as an archive filename".into(),
        });
    }
    let mut out = String::with_capacity(id.len());
    for byte in id.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => out.push(byte as char),
            _ => {
                use std::fmt::Write as _;
                write!(out, "%{byte:02X}").expect("writing to a String cannot fail");
            }
        }
    }
    Ok(out)
}

/// Digest a file's full contents, streaming, and report the byte count seen.
///
/// Shared by [`FileArchiveStore::verify`] for both the archived copy and the
/// source log: they are re-digested exactly the same way, which is what makes
/// the four [`ArchiveIntegrity`] outcomes a comparison of two independently
/// trustworthy numbers rather than two different measurements.
fn digest_file(path: &Path) -> PortResult<(String, u64)> {
    let file = File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
    let mut reader = BufReader::with_capacity(256 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut total: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    Ok((hex(hasher.finish()), total))
}

/// Write one record's body and terminator, updating a running digest and byte
/// count the same way regardless of whether the bytes came from the source
/// untouched or from a transform's replacement.
fn write_record(
    writer: &mut impl Write,
    hasher: &mut Sha256,
    archived_bytes: &mut u64,
    body: &[u8],
    terminator: &[u8],
    path_for_errors: &Path,
) -> PortResult<()> {
    writer
        .write_all(body)
        .and_then(|_| writer.write_all(terminator))
        .map_err(|e| PortError::Io(format!("{}: {e}", path_for_errors.display())))?;
    hasher.update(body);
    hasher.update(terminator);
    *archived_bytes += (body.len() + terminator.len()) as u64;
    Ok(())
}

impl ArchiveStore for FileArchiveStore {
    fn root(&self) -> String {
        self.root.display().to_string()
    }

    fn ingest(
        &self,
        descriptor: &SessionDescriptor,
        transform: &dyn RecordTransform,
    ) -> PortResult<ArchiveEntry> {
        let dest_path = self.session_path(descriptor.agent, descriptor.id.as_str())?;
        let agent_dir = self.sessions_dir(descriptor.agent);
        fs::create_dir_all(&agent_dir)
            .map_err(|e| PortError::Io(format!("{}: {e}", agent_dir.display())))?;
        // Unique per process so two ingests racing on the same session id
        // never collide on the same temporary name.
        let tmp_path = agent_dir.join(format!(
            "{}.tmp-{}",
            dest_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("archive"),
            std::process::id()
        ));

        let source_path = Path::new(&descriptor.path);
        let source_file = File::open(source_path)
            .map_err(|e| PortError::Io(format!("{}: {e}", source_path.display())))?;
        let mut reader = BufReader::with_capacity(256 * 1024, source_file);

        let tmp_file = File::create(&tmp_path)
            .map_err(|e| PortError::Io(format!("{}: {e}", tmp_path.display())))?;
        let mut writer = BufWriter::with_capacity(256 * 1024, tmp_file);

        let mut source_hasher = Sha256::new();
        let mut archive_hasher = Sha256::new();
        let mut source_bytes: u64 = 0;
        let mut archived_bytes: u64 = 0;
        let mut records: u64 = 0;
        let mut redacted_records: u64 = 0;
        let mut redacted_values: u64 = 0;

        let mut raw: Vec<u8> = Vec::with_capacity(4096);
        loop {
            raw.clear();
            let read = reader
                .read_until(b'\n', &mut raw)
                .map_err(|e| PortError::Io(format!("{}: {e}", source_path.display())))?;
            if read == 0 {
                // EOF. A trailing newline on the previous record ended that
                // record; it does not begin an empty one here.
                break;
            }
            records += 1;
            source_hasher.update(&raw);
            source_bytes += raw.len() as u64;

            // Split on the `\n` only, so a CRLF record's `\r` stays part of
            // the body and re-joining body + terminator reproduces the
            // original bytes exactly.
            let (body, terminator): (&[u8], &[u8]) = if raw.last() == Some(&b'\n') {
                let at = raw.len() - 1;
                (&raw[..at], &raw[at..])
            } else {
                (&raw[..], &[])
            };

            match std::str::from_utf8(body) {
                Ok(text) => {
                    let (transformed, replaced) = transform.apply(text);
                    if replaced > 0 {
                        redacted_records += 1;
                        redacted_values += replaced as u64;
                    }
                    match transformed {
                        Cow::Borrowed(_) => write_record(
                            &mut writer,
                            &mut archive_hasher,
                            &mut archived_bytes,
                            body,
                            terminator,
                            &tmp_path,
                        )?,
                        Cow::Owned(owned) => write_record(
                            &mut writer,
                            &mut archive_hasher,
                            &mut archived_bytes,
                            owned.as_bytes(),
                            terminator,
                            &tmp_path,
                        )?,
                    }
                }
                // Not decodable as UTF-8: copy verbatim rather than lose a
                // record we could not understand. An archive exists to keep
                // evidence; dropping what it cannot parse would defeat that.
                Err(_) => write_record(
                    &mut writer,
                    &mut archive_hasher,
                    &mut archived_bytes,
                    body,
                    terminator,
                    &tmp_path,
                )?,
            }
        }

        writer
            .flush()
            .map_err(|e| PortError::Io(format!("{}: {e}", tmp_path.display())))?;
        drop(writer);

        fs::rename(&tmp_path, &dest_path).map_err(|e| {
            let _ = fs::remove_file(&tmp_path);
            PortError::Io(format!(
                "{} -> {}: {e}",
                tmp_path.display(),
                dest_path.display()
            ))
        })?;

        let entry = ArchiveEntry {
            descriptor: descriptor.clone(),
            // `chrono`'s `clock` feature is not enabled for this crate (see
            // Cargo.toml / the workspace `chrono` entry), so `Utc::now()`
            // itself is unavailable. `DateTime<Utc>: From<SystemTime>` needs
            // only `std`, which is enabled, and `claude_code::mod` already
            // relies on that same conversion for `last_activity` -- so this
            // takes the current time from `SystemTime::now()` and converts,
            // rather than reading a source-file timestamp that would be wrong
            // (it answers "when was the source written", not "when was this
            // archived").
            archived_at: DateTime::<Utc>::from(SystemTime::now()),
            redaction: transform.mode(),
            records,
            source_bytes,
            source_digest: hex(source_hasher.finish()),
            archived_bytes,
            archived_digest: hex(archive_hasher.finish()),
            redacted_records,
            redacted_values,
        };

        self.append_to_manifest(&entry)?;
        Ok(entry)
    }

    fn entries(&self) -> PortResult<Vec<ArchiveEntry>> {
        let manifest_path = self.manifest_path();
        let bytes = match fs::read(&manifest_path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(PortError::Io(format!("{}: {e}", manifest_path.display()))),
        };

        let mut latest: HashMap<(AgentKind, String), ArchiveEntry> = HashMap::new();
        for mut line in bytes.split(|&b| b == b'\n') {
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if line.is_empty() {
                continue;
            }
            // A trailing partial write -- a crash mid-append, or a line cut
            // off inside a multi-byte character -- must cost only that line,
            // never the rest of the archive. Both a UTF-8 failure and a JSON
            // failure are therefore skipped rather than propagated.
            let Ok(text) = std::str::from_utf8(line) else {
                continue;
            };
            let Ok(entry) = serde_json::from_str::<ArchiveEntry>(text) else {
                continue;
            };
            latest.insert((entry.agent(), entry.id().as_str().to_string()), entry);
        }

        Ok(latest.into_values().collect())
    }

    fn entry(&self, agent: AgentKind, id: &str) -> PortResult<Option<ArchiveEntry>> {
        Ok(self
            .entries()?
            .into_iter()
            .find(|e| e.agent() == agent && e.id().as_str() == id))
    }

    fn verify(&self, agent: AgentKind, id: &str) -> PortResult<ArchiveIntegrity> {
        let entry = self
            .entry(agent, id)?
            .ok_or_else(|| PortError::NotFound(format!("{agent} session {id} in this archive")))?;

        // Checked first regardless of what the source looks like: a damaged
        // copy is a fact about this file, not about the world outside it.
        let archive_path = self.session_path(agent, id)?;
        let (current_archive_digest, _) = digest_file(&archive_path)?;
        if current_archive_digest != entry.archived_digest {
            return Ok(ArchiveIntegrity::ArchiveDamaged {
                recorded_digest: entry.archived_digest,
                current_digest: current_archive_digest,
            });
        }

        let source_path = Path::new(&entry.descriptor.path);
        if !source_path.exists() {
            return Ok(ArchiveIntegrity::SourceGone {
                archive_matches_digest: true,
            });
        }

        let (current_source_digest, current_source_bytes) = digest_file(source_path)?;
        if current_source_digest != entry.source_digest {
            return Ok(ArchiveIntegrity::SourceChanged {
                recorded_digest: entry.source_digest,
                current_digest: current_source_digest,
                recorded_bytes: entry.source_bytes,
                current_bytes: current_source_bytes,
            });
        }

        Ok(ArchiveIntegrity::Intact)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::archive::RedactionMode;
    use ct_domain::{SessionId, ThreadRole};

    /// Borrows unconditionally: the identity transform, and what `raw`
    /// retention uses in the application layer.
    struct Identity;
    impl RecordTransform for Identity {
        fn apply<'a>(&self, record: &'a str) -> (Cow<'a, str>, u32) {
            (Cow::Borrowed(record), 0)
        }
        fn mode(&self) -> RedactionMode {
            RedactionMode::Raw
        }
    }

    /// Replaces every occurrence of `SECRET` with a fixed placeholder,
    /// counting the replacements it made.
    struct ReplaceSecret;
    impl RecordTransform for ReplaceSecret {
        fn apply<'a>(&self, record: &'a str) -> (Cow<'a, str>, u32) {
            let count = record.matches("SECRET").count() as u32;
            if count == 0 {
                (Cow::Borrowed(record), 0)
            } else {
                (Cow::Owned(record.replace("SECRET", "[redacted]")), count)
            }
        }
        fn mode(&self) -> RedactionMode {
            RedactionMode::Redacted
        }
    }

    fn scratch_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("ct-archive-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        File::create(&path).unwrap().write_all(bytes).unwrap();
        path
    }

    fn descriptor(id: &str, source_path: &Path) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new(id).expect("a non-blank id"),
            agent: AgentKind::Codex,
            path: source_path.display().to_string(),
            size_bytes: 0,
            project: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        }
    }

    #[test]
    fn an_identity_transform_archives_bytes_identical_to_the_source() {
        let scratch = scratch_root("byte-identity");
        let source = write_source(
            &scratch,
            "source.jsonl",
            b"{\"a\":1}\n{\"b\":2}\r\n{\"c\":3}",
        );
        let store = FileArchiveStore::at(scratch.join("archive"));
        let entry = store
            .ingest(&descriptor("s1", &source), &Identity)
            .expect("ingest of a well-formed fixture must succeed");

        assert_eq!(entry.records, 3, "the unterminated final line still counts");
        assert_eq!(entry.redacted_records, 0);
        assert_eq!(entry.redacted_values, 0);
        assert_eq!(entry.archived_bytes, entry.source_bytes);
        assert_eq!(entry.archived_digest, entry.source_digest);

        let archived_path = scratch
            .join("archive")
            .join("sessions")
            .join("codex")
            .join("s1.jsonl");
        let archived_bytes = fs::read(&archived_path).unwrap();
        let source_bytes = fs::read(&source).unwrap();
        assert_eq!(
            archived_bytes, source_bytes,
            "an identity transform must produce a byte-identical copy, CRLF and all"
        );

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_non_utf8_record_survives_ingest_verbatim_and_is_counted() {
        let scratch = scratch_root("non-utf8");
        let mut bytes = b"{\"a\":1}\n".to_vec();
        bytes.extend_from_slice(&[0xFF, 0xFE, 0x00, b'\n']);
        bytes.extend_from_slice(b"{\"c\":3}\n");
        let source = write_source(&scratch, "source.jsonl", &bytes);

        let store = FileArchiveStore::at(scratch.join("archive"));
        let entry = store
            .ingest(&descriptor("s1", &source), &Identity)
            .expect("a non-UTF-8 record must not fail the whole ingest");

        assert_eq!(entry.records, 3, "the undecodable record is still counted");
        assert_eq!(entry.redacted_records, 0);

        let archived_path = scratch
            .join("archive")
            .join("sessions")
            .join("codex")
            .join("s1.jsonl");
        assert_eq!(
            fs::read(&archived_path).unwrap(),
            bytes,
            "the invalid bytes must be copied verbatim, not dropped or replaced"
        );

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn re_ingesting_the_same_session_is_idempotent() {
        let scratch = scratch_root("idempotent");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n{\"b\":2}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));

        let first = store.ingest(&descriptor("s1", &source), &Identity).unwrap();
        let second = store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        assert_eq!(
            first.archived_digest, second.archived_digest,
            "ingesting an unchanged source twice must produce identical bytes"
        );

        let entries = store.entries().unwrap();
        let matching: Vec<_> = entries
            .iter()
            .filter(|e| e.agent() == AgentKind::Codex && e.id().as_str() == "s1")
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "the append-only manifest must still expose exactly one entry per session"
        );

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_session_id_cannot_escape_the_archive_root() {
        let scratch = scratch_root("traversal");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));

        // Under a naive `agent_dir.join(id)`, three ".." components walk back
        // out of `sessions/<agent>/` and the archive root itself, landing the
        // file in `scratch` -- one level above where the archive is allowed
        // to write anything.
        let unsafe_id = "../../../ct-adapters-traversal-marker";
        let marker_outside_root = scratch.join("ct-adapters-traversal-marker.jsonl");
        let _ = fs::remove_file(&marker_outside_root);

        let result = store.ingest(&descriptor(unsafe_id, &source), &Identity);
        assert!(
            result.is_ok(),
            "an id with path-like characters is sanitised, not refused outright"
        );
        assert!(
            !marker_outside_root.exists(),
            "the id must not be able to write outside the archive root"
        );

        // Also reject the ids that are indistinguishable from "the archive
        // root itself" or "its parent" once turned into a bare path component.
        assert!(store.ingest(&descriptor(".", &source), &Identity).is_err());
        assert!(store.ingest(&descriptor("..", &source), &Identity).is_err());

        let _ = fs::remove_file(&marker_outside_root);
        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn verify_reports_intact_for_an_unchanged_session() {
        let scratch = scratch_root("verify-intact");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        assert_eq!(
            store.verify(AgentKind::Codex, "s1").unwrap(),
            ArchiveIntegrity::Intact
        );

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn verify_reports_source_changed_when_the_log_grew_after_ingest() {
        let scratch = scratch_root("verify-source-changed");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        let entry = store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        let mut f = OpenOptions::new().append(true).open(&source).unwrap();
        f.write_all(b"{\"b\":2}\n").unwrap();
        drop(f);

        match store.verify(AgentKind::Codex, "s1").unwrap() {
            ArchiveIntegrity::SourceChanged {
                recorded_digest,
                current_digest,
                recorded_bytes,
                current_bytes,
            } => {
                assert_eq!(recorded_digest, entry.source_digest);
                assert_ne!(current_digest, entry.source_digest);
                assert_eq!(recorded_bytes, entry.source_bytes);
                assert!(current_bytes > recorded_bytes);
            }
            other => panic!("expected SourceChanged, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn verify_reports_source_gone_after_the_log_is_deleted() {
        let scratch = scratch_root("verify-source-gone");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        fs::remove_file(&source).unwrap();

        assert_eq!(
            store.verify(AgentKind::Codex, "s1").unwrap(),
            ArchiveIntegrity::SourceGone {
                archive_matches_digest: true
            }
        );

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn verify_reports_archive_damaged_when_the_copy_no_longer_matches_its_digest() {
        let scratch = scratch_root("verify-damaged");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        let entry = store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        let archived_path = scratch
            .join("archive")
            .join("sessions")
            .join("codex")
            .join("s1.jsonl");
        fs::write(&archived_path, b"corrupted").unwrap();

        match store.verify(AgentKind::Codex, "s1").unwrap() {
            ArchiveIntegrity::ArchiveDamaged {
                recorded_digest,
                current_digest,
            } => {
                assert_eq!(recorded_digest, entry.archived_digest);
                assert_ne!(current_digest, entry.archived_digest);
            }
            other => panic!("expected ArchiveDamaged, got {other:?}"),
        }

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn verify_reports_not_found_only_when_the_archive_never_held_the_session() {
        let scratch = scratch_root("verify-not-found");
        let store = FileArchiveStore::at(scratch.join("archive"));

        let err = store
            .verify(AgentKind::Codex, "never-ingested")
            .unwrap_err();
        assert!(matches!(err, PortError::NotFound(_)));

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_truncated_final_manifest_line_is_skipped_and_earlier_entries_still_load() {
        let scratch = scratch_root("truncated-manifest");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        // Simulate a crash mid-append: a second, well-formed line followed by
        // a chunk that stops partway through its JSON.
        let manifest_path = scratch.join("archive").join("manifest.ndjson");
        let mut f = OpenOptions::new()
            .append(true)
            .open(&manifest_path)
            .unwrap();
        f.write_all(b"{\"descriptor\":{\"id\":\"s2\",\"agent\":\"cod")
            .unwrap();
        drop(f);

        let entries = store.entries().unwrap();
        assert_eq!(
            entries.len(),
            1,
            "the truncated line must be skipped, not error"
        );
        assert_eq!(entries[0].id().as_str(), "s1");

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn a_redacting_transform_reports_matching_counts_and_a_changed_byte_length() {
        let scratch = scratch_root("redacted-counts");
        let source = write_source(
            &scratch,
            "source.jsonl",
            b"token=SECRET\nno secret here\nSECRET and SECRET again\n",
        );
        let store = FileArchiveStore::at(scratch.join("archive"));
        let entry = store
            .ingest(&descriptor("s1", &source), &ReplaceSecret)
            .unwrap();

        assert_eq!(entry.records, 3);
        assert_eq!(
            entry.redacted_records, 2,
            "two of the three records contain SECRET"
        );
        assert_eq!(
            entry.redacted_values, 3,
            "three total occurrences of SECRET across those records"
        );
        assert_eq!(entry.redaction, RedactionMode::Redacted);
        assert_ne!(
            entry.archived_bytes, entry.source_bytes,
            "[redacted] is a different length than SECRET"
        );
        assert!(entry.differs_from_source());

        let _ = fs::remove_dir_all(&scratch);
    }
}
