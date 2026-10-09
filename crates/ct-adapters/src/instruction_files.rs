//! Bounded local-file reader for explicit instruction comparisons.
use ct_domain::local_paths::{canonical_local_path, local_absolute};
use ct_domain::ports::{InstructionFileReader, InstructionReadError};
use std::fs::{File, Metadata, OpenOptions};
use std::io::Read;
use std::path::{Path, PathBuf};

/// Instruction bodies larger than 4 MiB are refused rather than loaded/hashed.
pub const MAX_INSTRUCTION_BYTES: u64 = 4 * 1024 * 1024;
pub struct LocalInstructionFileReader;

impl InstructionFileReader for LocalInstructionFileReader {
    fn read(&self, path: &Path) -> Result<Vec<u8>, InstructionReadError> {
        if !local_absolute(&path.to_string_lossy()) || !local_drive(path) {
            return Err(InstructionReadError::UnsafePath);
        }
        // Inspect each component without following it. This rejects junctions
        // and links before canonicalize could traverse to a share/device.
        reject_links(path)?;
        let canonical = canonical_local_path(&std::fs::canonicalize(path)?);
        if !local_absolute(&canonical.to_string_lossy()) || !local_drive(&canonical) {
            return Err(InstructionReadError::UnsafePath);
        }
        reject_links(&canonical)?;
        let mut options = OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            // Do not follow a final reparse point swapped in since validation.
            options.custom_flags(0x0020_0000); // FILE_FLAG_OPEN_REPARSE_POINT
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        let file = options.open(&canonical)?;
        let metadata = file.metadata()?;
        if is_link(&metadata) {
            return Err(InstructionReadError::UnsafePath);
        }
        read_bounded(file, &metadata, MAX_INSTRUCTION_BYTES)
    }
}

fn read_bounded(
    file: File,
    metadata: &Metadata,
    limit: u64,
) -> Result<Vec<u8>, InstructionReadError> {
    if !metadata.is_file() {
        return Err(InstructionReadError::NotRegular);
    }
    if metadata.len() > limit {
        return Err(InstructionReadError::TooLarge);
    }
    let mut bytes = Vec::new();
    // A concurrent append cannot bypass the metadata size check.
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(InstructionReadError::TooLarge);
    }
    Ok(bytes)
}

fn reject_links(path: &Path) -> Result<(), InstructionReadError> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        prefix.push(component);
        if !prefix.has_root() {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&prefix)?;
        if is_link(&metadata) {
            return Err(InstructionReadError::UnsafePath);
        }
    }
    let metadata = std::fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(InstructionReadError::NotRegular);
    }
    Ok(())
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0 // FILE_ATTRIBUTE_REPARSE_POINT
    }
    #[cfg(not(windows))]
    {
        metadata.file_type().is_symlink()
    }
}

#[cfg(windows)]
fn local_drive(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    let Some(root) = path.ancestors().last() else {
        return false;
    };
    let mut wide: Vec<u16> = root.as_os_str().encode_wide().collect();
    wide.push(0);
    // SAFETY: wide is a live NUL-terminated UTF-16 string; this only queries
    // the root's drive category, before any instruction path is traversed.
    matches!(unsafe { GetDriveTypeW(wide.as_ptr()) }, 2 | 3 | 5 | 6)
}

#[cfg(not(windows))]
fn local_drive(_path: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "ct-instruction-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn unsafe_names_are_refused_before_any_file_access() {
        for value in [
            r"\\server\share\AGENTS.md",
            "//server/share/file",
            r"\\?\C:\file",
            r"\\.\pipe\input",
            "C:relative",
            "C:/file:stream",
            "AGENTS.md",
            "NUL",
        ] {
            assert!(
                matches!(
                    LocalInstructionFileReader.read(Path::new(value)),
                    Err(InstructionReadError::UnsafePath)
                ),
                "{value}"
            );
        }
    }

    #[test]
    fn regular_local_files_read_but_directories_and_large_files_refuse() {
        let temp = Temp::new();
        let file = temp.0.join("AGENTS.md");
        std::fs::write(&file, b"local instruction").unwrap();
        assert_eq!(
            LocalInstructionFileReader.read(&file).unwrap(),
            b"local instruction"
        );
        assert!(matches!(
            LocalInstructionFileReader.read(&temp.0),
            Err(InstructionReadError::NotRegular)
        ));
        OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_len(MAX_INSTRUCTION_BYTES + 1)
            .unwrap();
        assert!(matches!(
            LocalInstructionFileReader.read(&file),
            Err(InstructionReadError::TooLarge)
        ));
    }

    #[test]
    fn concurrent_growth_cannot_exceed_read_limit() {
        let temp = Temp::new();
        let file = temp.0.join("AGENTS.md");
        std::fs::write(&file, b"old").unwrap();
        let handle = File::open(&file).unwrap();
        let before = handle.metadata().unwrap();
        std::fs::write(&file, b"larger than limit").unwrap();
        assert!(matches!(
            read_bounded(handle, &before, 4),
            Err(InstructionReadError::TooLarge)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn links_and_fifos_are_refused_without_opening() {
        use std::os::unix::ffi::OsStrExt;
        let temp = Temp::new();
        let target = temp.0.join("AGENTS.md");
        std::fs::write(&target, b"local").unwrap();
        let link = temp.0.join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(matches!(
            LocalInstructionFileReader.read(&link),
            Err(InstructionReadError::UnsafePath)
        ));
        let fifo = temp.0.join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_bytes()).unwrap();
        // SAFETY: name is NUL terminated and refers only to this test directory.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert!(matches!(
            LocalInstructionFileReader.read(&fifo),
            Err(InstructionReadError::NotRegular)
        ));
    }
    #[cfg(windows)]
    #[test]
    fn parent_junction_is_refused_before_reading_target() {
        use std::os::windows::process::CommandExt;
        let temp = Temp::new();
        let target = temp.0.join("target");
        std::fs::create_dir(&target).unwrap();
        std::fs::write(target.join("AGENTS.md"), b"local fixture").unwrap();
        let junction = temp.0.join("junction");
        let output = std::process::Command::new("cmd")
            .args(["/D", "/C", "mklink", "/J"])
            .arg(&junction)
            .arg(&target)
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "junction fixture creation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result = LocalInstructionFileReader.read(&junction.join("AGENTS.md"));
        std::fs::remove_dir(&junction).unwrap();
        assert!(matches!(result, Err(InstructionReadError::UnsafePath)));
    }
}
