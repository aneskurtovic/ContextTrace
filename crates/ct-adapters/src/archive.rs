//! Filesystem implementation of [`ArchiveStore`].
//!
//! Layout, chosen so a crash costs at most the record or manifest line being
//! written when it happens:
//!
//! ```text
//! <root>/manifest.ndjson              one ArchiveEntry as JSON per line, appended
//! <root>/sessions/<agent>/<id>.<digest>.jsonl  immutable copied records
//! <root>/sessions/<agent>/<id>.jsonl           legacy copies (still readable)
//! <root>/writer.lock                         OS-held writer coordination
//! ```
//!
//! The manifest is append-only and last-wins: re-ingesting a session appends a
//! new line rather than rewriting the file, so [`FileArchiveStore::entries`]
//! keeps the last entry per `(agent, id)` and silently skips anything it
//! cannot parse -- a trailing partial line from a crash mid-append included.
//! Writers separate an unterminated tail before appending and sync copied bytes
//! before publishing their manifest reference. Digest-named copies never replace
//! a prior revision: failed metadata commits leave the last acknowledged pair
//! readable. Unreferenced revisions are retained and can be reused on retry; no
//! garbage collection deletes potentially recoverable evidence.

use crate::fingerprint::{hex, Sha256};
use crate::home_dir;
use chrono::{DateTime, Utc};
use ct_domain::model::archive::{ArchiveEntry, ArchiveIntegrity};
use ct_domain::ports::{ArchiveStore, PortError, PortResult, RecordTransform};
use ct_domain::{AgentKind, SessionDescriptor, SessionId};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

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

    fn copy_path(&self, entry: &ArchiveEntry) -> PortResult<PathBuf> {
        let legacy = self.session_path(entry.agent(), entry.id().as_str())?;
        if !entry.versioned_copy {
            return Ok(legacy);
        }
        if entry.archived_digest.len() != 64
            || !entry.archived_digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(PortError::Malformed {
                path: self.manifest_path().display().to_string(),
                detail: "invalid archive copy digest".into(),
            });
        }
        let id = safe_filename(entry.id().as_str())?;
        Ok(self
            .sessions_dir(entry.agent())
            .join(format!("{id}.{}.jsonl", entry.archived_digest)))
    }

    /// The OS releases this lock when the file closes, including process crashes.
    /// Every instance/process writing this root uses the same lock file.
    fn writer_lock(&self) -> PortResult<File> {
        fs::create_dir_all(&self.root).map_err(|e| io_error(&self.root, e))?;
        let path = self.root.join("writer.lock");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let mut options = OpenOptions::new();
            options.create(true).truncate(false).read(true).write(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                options.share_mode(0);
            }
            match options.open(&path) {
                Ok(file) => {
                    #[cfg(unix)]
                    {
                        use std::os::fd::AsRawFd;
                        // SAFETY: the descriptor belongs to the live File, and flock
                        // neither retains a pointer nor changes file ownership.
                        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) }
                            != 0
                        {
                            let error = std::io::Error::last_os_error();
                            if error.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline
                            {
                                std::thread::sleep(Duration::from_millis(20));
                                continue;
                            }
                            return Err(io_error(&path, error));
                        }
                    }
                    return Ok(file);
                }
                Err(error) => {
                    #[cfg(windows)]
                    if error.raw_os_error() == Some(32) && Instant::now() < deadline {
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    return Err(io_error(&path, error));
                }
            }
        }
    }

    /// Caller holds writer_lock. Preserve an interrupted tail as its own line,
    /// so it cannot swallow the next acknowledged entry.
    fn append_to_manifest(&self, entry: &ArchiveEntry) -> PortResult<()> {
        let path = self.manifest_path();
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| PortError::Io(format!("serialising archive entry: {e}")))?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&path)
            .map_err(|e| io_error(&path, e))?;
        append_manifest_line(&mut file, &line).map_err(|e| io_error(&path, e))?;
        file.sync_all().map_err(|e| io_error(&path, e))?;
        #[cfg(unix)]
        File::open(&self.root)
            .and_then(|dir| dir.sync_all())
            .map_err(|e| io_error(&self.root, e))?;
        Ok(())
    }
}

fn io_error(path: &Path, error: std::io::Error) -> PortError {
    PortError::Io(format!("{}: {error}", path.display()))
}

/// Kept generic so partial writes can be fault-injected without filesystem permissions.
fn append_manifest_line(file: &mut (impl Read + Write + Seek), line: &[u8]) -> std::io::Result<()> {
    let length = file.seek(SeekFrom::End(0))?;
    if length > 0 {
        file.seek(SeekFrom::End(-1))?;
        let mut last = [0];
        file.read_exact(&mut last)?;
        file.seek(SeekFrom::End(0))?;
        if last[0] != b'\n' {
            file.write_all(b"\n")?;
        }
    }
    file.write_all(line)
}

struct PendingCopy {
    path: PathBuf,
}
impl PendingCopy {
    fn create(dir: &Path, stem: &str) -> PortResult<(Self, File)> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        for _ in 0..128 {
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos();
            let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
            let path = dir.join(format!(
                "{stem}.tmp-{}-{timestamp}-{nonce}",
                std::process::id()
            ));
            match OpenOptions::new().create_new(true).write(true).open(&path) {
                Ok(file) => return Ok((Self { path }, file)),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io_error(&path, error)),
            }
        }
        Err(PortError::Io(
            "could not allocate unique archive scratch file".into(),
        ))
    }
}
impl Drop for PendingCopy {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

impl Default for FileArchiveStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Directory created under `%LOCALAPPDATA%` for archived sessions.
///
/// A *sibling* of the desktop app's install directory, never a child of it.
/// See [`default_root`].
const WINDOWS_ARCHIVE_DIR: &str = "ContextTrace-archive";

/// `%LOCALAPPDATA%\ContextTrace-archive` on Windows, `$XDG_DATA_HOME` or
/// `~/.local/share` elsewhere. Checked in this order without `cfg(windows)`
/// gating, matching [`crate::home_dir`]'s style: `LOCALAPPDATA` is simply
/// unset on platforms where it does not apply, so the fallbacks are reached
/// there without needing a compile-time split.
///
/// # Why not `%LOCALAPPDATA%\ContextTrace\archive`
///
/// Because that is the desktop app's **install directory**. Tauri's NSIS
/// bundler installs per-user to `%LOCALAPPDATA%\<productName>`, so an archive
/// nested there would be user data living inside a program directory an
/// uninstaller owns.
///
/// It happens to survive today: the generated uninstaller ends with
/// `RMDir "$INSTDIR"`, which removes the directory only when it is empty, so an
/// archive subdirectory silently blocks it. That is a detail of a template this
/// project does not control, one `RMDir /r` away from uninstalling the app
/// deleting the only remaining copies of sessions whose logs are already gone --
/// the exact loss this whole feature exists to prevent, caused by a routine
/// dependency bump. A sibling directory cannot be reached by that mistake.
///
/// **Only the Windows branch is flat, and deliberately so.** Windows puts a
/// per-user install *and* per-user data under the same `%LOCALAPPDATA%` root, so
/// `<app>/archive` collides there; XDG keeps them apart, and
/// `~/.local/share/contexttrace/archive` is a data directory no uninstaller
/// owns. The rule is "never nest inside an install directory", not "never
/// nest" -- so it changes the layout on exactly the platform where the two
/// would otherwise be the same place.
fn default_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("CONTEXTTRACE_ARCHIVE") {
        return PathBuf::from(dir);
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(local).join(WINDOWS_ARCHIVE_DIR);
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
/// log directory to. [`SessionId::file_stem`] holds that rule, because the
/// desktop's export path needs the same one and a session should not appear
/// under two different stems in two subdirectories of the same root.
fn safe_filename(id: &str) -> PortResult<String> {
    SessionId::new(id)
        .ok()
        .and_then(|id| id.file_stem())
        .ok_or_else(|| PortError::Malformed {
            path: id.to_string(),
            detail: "session id is not safe to use as an archive filename".into(),
        })
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
        let stem = safe_filename(descriptor.id.as_str())?;
        let agent_dir = self.sessions_dir(descriptor.agent);
        fs::create_dir_all(&agent_dir).map_err(|e| io_error(&agent_dir, e))?;
        let (pending, tmp_file) = PendingCopy::create(&agent_dir, &stem)?;
        let tmp_path = &pending.path;

        let source_path = Path::new(&descriptor.path);
        let source_file = File::open(source_path)
            .map_err(|e| PortError::Io(format!("{}: {e}", source_path.display())))?;
        let mut reader = BufReader::with_capacity(256 * 1024, source_file);

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
                            tmp_path,
                        )?,
                        Cow::Owned(owned) => write_record(
                            &mut writer,
                            &mut archive_hasher,
                            &mut archived_bytes,
                            owned.as_bytes(),
                            terminator,
                            tmp_path,
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
                    tmp_path,
                )?,
            }
        }

        writer
            .flush()
            .map_err(|e| PortError::Io(format!("{}: {e}", tmp_path.display())))?;
        writer
            .get_ref()
            .sync_all()
            .map_err(|e| io_error(tmp_path, e))?;
        drop(writer);

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
            versioned_copy: true,
            redacted_records,
            redacted_values,
        };

        let _lock = self.writer_lock()?;
        let dest_path = self.copy_path(&entry)?;
        if dest_path.exists() {
            // An identical revision may be reused after a failed manifest commit.
            let (digest, bytes) = digest_file(&dest_path)?;
            if digest != entry.archived_digest || bytes != entry.archived_bytes {
                return Err(PortError::Io(format!(
                    "{}: existing immutable copy is damaged",
                    dest_path.display()
                )));
            }
        } else {
            fs::rename(tmp_path, &dest_path).map_err(|e| io_error(&dest_path, e))?;
            #[cfg(unix)]
            File::open(&agent_dir)
                .and_then(|dir| dir.sync_all())
                .map_err(|e| io_error(&agent_dir, e))?;
        }
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

    fn path(&self, agent: AgentKind, id: &str) -> PortResult<Option<String>> {
        let Some(entry) = self.entry(agent, id)? else {
            return Ok(None);
        };
        let path = self.copy_path(&entry)?;
        match fs::metadata(&path) {
            Ok(_) => Ok(Some(path.display().to_string())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(PortError::Io(format!("{}: {error}", path.display()))),
        }
    }

    fn verify(&self, agent: AgentKind, id: &str) -> PortResult<ArchiveIntegrity> {
        let entry = self
            .entry(agent, id)?
            .ok_or_else(|| PortError::NotFound(format!("{agent} session {id} in this archive")))?;

        // Checked first regardless of what the source looks like: a damaged
        // copy is a fact about this file, not about the world outside it.
        let archive_path = self.copy_path(&entry)?;
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
            title: None,
            git_branch: None,
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

        let archived_path = PathBuf::from(store.path(AgentKind::Codex, "s1").unwrap().unwrap());
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

        let archived_path = PathBuf::from(store.path(AgentKind::Codex, "s1").unwrap().unwrap());
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
    fn path_returns_the_archived_copy_even_after_the_source_is_gone() {
        let scratch = scratch_root("path-after-source-gone");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        fs::remove_file(&source).unwrap();

        let path = store
            .path(AgentKind::Codex, "s1")
            .unwrap()
            .expect("the archive copy remains addressable");
        assert_eq!(fs::read(path).unwrap(), b"{\"a\":1}\n");

        let _ = fs::remove_dir_all(&scratch);
    }

    #[test]
    fn verify_reports_archive_damaged_when_the_copy_no_longer_matches_its_digest() {
        let scratch = scratch_root("verify-damaged");
        let source = write_source(&scratch, "source.jsonl", b"{\"a\":1}\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        let entry = store.ingest(&descriptor("s1", &source), &Identity).unwrap();

        let archived_path = PathBuf::from(store.path(AgentKind::Codex, "s1").unwrap().unwrap());
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

    /// The archive must never default to living inside the desktop app's
    /// install directory.
    ///
    /// Tauri installs per-user to `%LOCALAPPDATA%\<productName>`, so that
    /// directory belongs to an uninstaller. The generated uninstaller currently
    /// ends in `RMDir "$INSTDIR"`, which spares a non-empty directory -- but
    /// that is a detail of a template this project does not control, and one
    /// `RMDir /r` away from an uninstall deleting the only surviving copies of
    /// sessions whose logs are already gone. Pinned so a tidier-looking default
    /// cannot reintroduce the nesting quietly.
    #[test]
    fn the_windows_archive_directory_never_nests_inside_the_install_directory() {
        // Tauri's `productName`, which is the install directory's name. Scoped
        // to Windows on purpose: it is the platform that puts a per-user
        // install and per-user data under one root, so it is the only one where
        // the conventional `<app>/archive` layout would collide.
        const DESKTOP_INSTALL_DIR: &str = "ContextTrace";

        assert_ne!(WINDOWS_ARCHIVE_DIR, DESKTOP_INSTALL_DIR);
        assert!(
            !Path::new(WINDOWS_ARCHIVE_DIR).starts_with(DESKTOP_INSTALL_DIR),
            "{WINDOWS_ARCHIVE_DIR} must not sit under {DESKTOP_INSTALL_DIR}"
        );
    }
    #[test]
    fn an_interrupted_manifest_tail_cannot_hide_the_next_successful_archive() {
        for existing in [false, true] {
            let scratch = scratch_root(if existing {
                "tail-existing"
            } else {
                "tail-new"
            });
            let source = write_source(&scratch, "source.jsonl", b"version one\n");
            let archive = scratch.join("archive");
            let store = FileArchiveStore::at(&archive);
            if existing {
                store.ingest(&descriptor("s1", &source), &Identity).unwrap();
            }
            fs::create_dir_all(&archive).unwrap();
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(store.manifest_path())
                .unwrap()
                .write_all(b"{partial")
                .unwrap();
            fs::write(&source, b"version two\n").unwrap();
            let expected = store.ingest(&descriptor("s1", &source), &Identity).unwrap();
            let restarted = FileArchiveStore::at(&archive);
            assert_eq!(
                restarted.entry(AgentKind::Codex, "s1").unwrap(),
                Some(expected)
            );
            assert_eq!(
                restarted.verify(AgentKind::Codex, "s1").unwrap(),
                ArchiveIntegrity::Intact
            );
            assert!(fs::read_to_string(store.manifest_path())
                .unwrap()
                .contains("{partial\n"));
            fs::remove_dir_all(scratch).unwrap();
        }
    }

    #[test]
    fn failed_metadata_commit_preserves_the_previous_copy_and_retry_adopts_the_orphan() {
        let scratch = scratch_root("metadata-failure");
        let source = write_source(&scratch, "source.jsonl", b"old acknowledged bytes\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        let old = store.ingest(&descriptor("s1", &source), &Identity).unwrap();
        let old_path = store.copy_path(&old).unwrap();
        let saved = store.root.join("manifest.saved");
        fs::rename(store.manifest_path(), &saved).unwrap();
        fs::create_dir(store.manifest_path()).unwrap();
        fs::write(&source, b"new uncommitted bytes\n").unwrap();
        assert!(store.ingest(&descriptor("s1", &source), &Identity).is_err());
        fs::remove_dir(store.manifest_path()).unwrap();
        fs::rename(saved, store.manifest_path()).unwrap();
        let restarted = FileArchiveStore::at(&store.root);
        assert_eq!(
            restarted.entry(AgentKind::Codex, "s1").unwrap(),
            Some(old.clone())
        );
        assert_eq!(fs::read(&old_path).unwrap(), b"old acknowledged bytes\n");
        assert!(matches!(
            restarted.verify(AgentKind::Codex, "s1").unwrap(),
            ArchiveIntegrity::SourceChanged { .. }
        ));
        let before = fs::read_dir(store.sessions_dir(AgentKind::Codex))
            .unwrap()
            .count();
        assert_eq!(
            before, 2,
            "the uncommitted immutable revision remains recoverable"
        );
        let new = restarted
            .ingest(&descriptor("s1", &source), &Identity)
            .unwrap();
        assert_ne!(new.archived_digest, old.archived_digest);
        assert_eq!(
            fs::read_dir(store.sessions_dir(AgentKind::Codex))
                .unwrap()
                .count(),
            before
        );
        assert_eq!(
            restarted.verify(AgentKind::Codex, "s1").unwrap(),
            ArchiveIntegrity::Intact
        );
        assert_eq!(fs::read(old_path).unwrap(), b"old acknowledged bytes\n");
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn partial_manifest_write_preserves_history_and_the_following_record_boundary() {
        struct FailAfter {
            inner: std::io::Cursor<Vec<u8>>,
            left: usize,
        }
        impl Read for FailAfter {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                self.inner.read(buf)
            }
        }
        impl Seek for FailAfter {
            fn seek(&mut self, at: SeekFrom) -> std::io::Result<u64> {
                self.inner.seek(at)
            }
        }
        impl Write for FailAfter {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                if self.left == 0 {
                    return Err(std::io::Error::other("injected manifest write failure"));
                }
                let n = buf.len().min(self.left);
                self.left -= n;
                self.inner.write(&buf[..n])
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let original = b"{\"old\":true}\n".to_vec();
        let mut file = FailAfter {
            inner: std::io::Cursor::new(original.clone()),
            left: 7,
        };
        assert!(append_manifest_line(&mut file, b"{\"new\":true}\n").is_err());
        assert!(file.inner.get_ref().starts_with(&original));
        file.left = usize::MAX;
        append_manifest_line(&mut file, b"{\"retry\":true}\n").unwrap();
        let parsed: Vec<serde_json::Value> = file
            .inner
            .get_ref()
            .split(|&b| b == b'\n')
            .filter_map(|line| serde_json::from_slice(line).ok())
            .collect();
        assert_eq!(
            parsed,
            vec![
                serde_json::json!({"old":true}),
                serde_json::json!({"retry":true})
            ]
        );
    }

    #[test]
    fn legacy_manifest_and_copy_remain_readable_after_migration() {
        let scratch = scratch_root("legacy");
        let source = write_source(&scratch, "source.jsonl", b"legacy bytes\n");
        let store = FileArchiveStore::at(scratch.join("archive"));
        let mut entry = store.ingest(&descriptor("s1", &source), &Identity).unwrap();
        fs::rename(
            store.copy_path(&entry).unwrap(),
            store.session_path(AgentKind::Codex, "s1").unwrap(),
        )
        .unwrap();
        entry.versioned_copy = false;
        let mut legacy = serde_json::to_value(&entry).unwrap();
        legacy.as_object_mut().unwrap().remove("versioned_copy");
        fs::write(store.manifest_path(), format!("{legacy}\n")).unwrap();
        assert_eq!(
            store.verify(AgentKind::Codex, "s1").unwrap(),
            ArchiveIntegrity::Intact
        );
        assert_eq!(
            fs::read(store.path(AgentKind::Codex, "s1").unwrap().unwrap()).unwrap(),
            b"legacy bytes\n"
        );
        fs::write(&source, b"new revision\n").unwrap();
        store.ingest(&descriptor("s1", &source), &Identity).unwrap();
        assert_eq!(
            fs::read(store.session_path(AgentKind::Codex, "s1").unwrap()).unwrap(),
            b"legacy bytes\n"
        );
        fs::remove_dir_all(scratch).unwrap();
    }

    #[test]
    fn simultaneous_same_id_ingests_have_unique_scratch_and_coherent_copy_metadata() {
        let scratch = scratch_root("concurrent");
        let archive = scratch.join("archive");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let workers: Vec<_> = (0..8)
            .map(|index| {
                let source = write_source(
                    &scratch,
                    &format!("source-{index}.jsonl"),
                    format!("distinct version {index}\n").as_bytes(),
                );
                let store = FileArchiveStore::at(&archive);
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.ingest(&descriptor("s1", &source), &Identity).unwrap()
                })
            })
            .collect();
        let store = FileArchiveStore::at(&archive);
        for worker in workers {
            let entry = worker.join().unwrap();
            assert_eq!(
                digest_file(&store.copy_path(&entry).unwrap()).unwrap().0,
                entry.archived_digest
            );
        }
        assert_eq!(store.entries().unwrap().len(), 1);
        assert_eq!(
            store.verify(AgentKind::Codex, "s1").unwrap(),
            ArchiveIntegrity::Intact
        );
        assert_eq!(
            fs::read_dir(store.sessions_dir(AgentKind::Codex))
                .unwrap()
                .count(),
            8
        );
        fs::remove_dir_all(scratch).unwrap();
    }
    #[test]
    fn writer_lock_child_probe() {
        let Some(root) = std::env::var_os("CT_ARCHIVE_LOCK_TEST_ROOT") else {
            return;
        };
        let store = FileArchiveStore::at(root);
        fs::write(store.root.join("child.started"), b"started").unwrap();
        let _lock = store.writer_lock().unwrap();
        fs::write(store.root.join("child.acquired"), b"acquired").unwrap();
    }

    #[test]
    fn archive_writer_lock_serializes_a_separate_process() {
        let scratch = scratch_root("process-lock");
        let store = FileArchiveStore::at(scratch.join("archive"));
        let lock = store.writer_lock().unwrap();
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "archive::tests::writer_lock_child_probe",
                "--nocapture",
            ])
            .env("CT_ARCHIVE_LOCK_TEST_ROOT", &store.root)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while !store.root.join("child.started").exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let started = store.root.join("child.started").exists();
        std::thread::sleep(Duration::from_millis(100));
        let acquired_while_locked = store.root.join("child.acquired").exists();
        drop(lock);
        while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if child.try_wait().unwrap().is_none() {
            child.kill().unwrap();
        }
        let status = child.wait().unwrap();
        assert!(started, "child must start before testing exclusion");
        assert!(
            !acquired_while_locked,
            "another process cannot acquire the root while locked"
        );
        assert!(status.success());
        assert!(store.root.join("child.acquired").exists());
        fs::remove_dir_all(scratch).unwrap();
    }
}
