//! Filesystem implementation of [`RawEventSource`].
//!
//! This is the *only* place in ContextTrace that re-reads session bytes after
//! parsing, which is what makes the read-only guarantee auditable: there is one
//! file-access path to review, it opens files for reading, and it never seeks
//! with intent to write.

use ct_domain::ports::{PortError, PortResult, RawEventSource};
use ct_domain::{FileId, SourceRef};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Serves the original bytes behind a [`SourceRef`].
///
/// Within a loaded session, [`FileId`] `0` is by convention that session's own
/// file -- both supported agents keep one session per file. The map exists for
/// future adapters that split a session across several.
pub struct FileRawEventSource {
    files: HashMap<FileId, PathBuf>,
}

impl FileRawEventSource {
    /// The common case: one session, one file.
    pub fn for_session(path: impl Into<PathBuf>) -> Self {
        let mut files = HashMap::new();
        files.insert(FileId(0), path.into());
        Self { files }
    }

    pub fn with_files(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        Self {
            files: paths
                .into_iter()
                .enumerate()
                .map(|(i, p)| (FileId(i as u32), p))
                .collect(),
        }
    }

    fn path_for(&self, id: FileId) -> PortResult<&Path> {
        self.files
            .get(&id)
            .map(PathBuf::as_path)
            .ok_or_else(|| PortError::NotFound(format!("file id {}", id.0)))
    }
}

impl RawEventSource for FileRawEventSource {
    fn fetch(&self, source: SourceRef) -> PortResult<String> {
        let path = self.path_for(source.file)?;
        let mut file =
            File::open(path).map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;
        file.seek(SeekFrom::Start(source.byte_offset))
            .map_err(|e| PortError::Io(format!("{}: {e}", path.display())))?;

        let mut buf = vec![0u8; source.byte_len as usize];
        // `read_exact` rather than `read`: a short read means the file changed
        // underneath us (the agent appended, or the session was rotated), and
        // silently returning a truncated event would corrupt the raw inspector's
        // one job -- showing exactly what is on disk.
        file.read_exact(&mut buf).map_err(|e| {
            PortError::Io(format!(
                "{}: reading {} bytes at offset {}: {e}",
                path.display(),
                source.byte_len,
                source.byte_offset
            ))
        })?;

        // Lossy rather than strict: session logs occasionally carry invalid
        // UTF-8 from terminal output, and showing replacement characters beats
        // refusing to show the event at all.
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("ct-raw-test-{name}"));
        File::create(&path).unwrap().write_all(contents).unwrap();
        path
    }

    #[test]
    fn fetches_the_exact_byte_range() {
        let path = temp_file("exact.jsonl", b"{\"a\":1}\n{\"b\":2}\n");
        let src = FileRawEventSource::for_session(&path);

        let first = src.fetch(SourceRef::new(FileId(0), 0, 7, 1)).unwrap();
        assert_eq!(first, "{\"a\":1}");
        let second = src.fetch(SourceRef::new(FileId(0), 8, 7, 2)).unwrap();
        assert_eq!(second, "{\"b\":2}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn preview_truncates_without_reading_differently() {
        let path = temp_file("preview.jsonl", b"abcdefghij");
        let src = FileRawEventSource::for_session(&path);
        let preview = src
            .fetch_preview(SourceRef::new(FileId(0), 0, 10, 1), 4)
            .unwrap();
        assert_eq!(preview, "abcd\u{2026}");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_range_past_the_end_is_an_error_not_a_truncation() {
        let path = temp_file("short.jsonl", b"abc");
        let src = FileRawEventSource::for_session(&path);
        assert!(
            src.fetch(SourceRef::new(FileId(0), 0, 999, 1)).is_err(),
            "a short read means the file changed; that must surface"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn unknown_file_ids_are_reported() {
        let src = FileRawEventSource::for_session("nowhere");
        let err = src.fetch(SourceRef::new(FileId(7), 0, 1, 1)).unwrap_err();
        assert!(matches!(err, PortError::NotFound(_)));
    }

    #[test]
    fn invalid_utf8_is_shown_rather_than_refused() {
        let path = temp_file("badutf8.jsonl", &[0x61, 0xFF, 0x62]);
        let src = FileRawEventSource::for_session(&path);
        let text = src.fetch(SourceRef::new(FileId(0), 0, 3, 1)).unwrap();
        assert!(text.starts_with('a') && text.ends_with('b'));
        let _ = std::fs::remove_file(path);
    }
}
