//! Validate a release archive and extract it exactly as the Tauri macOS updater does.
use flate2::read::GzDecoder;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    env, fs, io,
    path::Path,
};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn validate(path: &Path) -> io::Result<()> {
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(path)?));
    let mut seen = HashSet::new();
    let mut links = HashMap::new();
    let mut found_file = false;
    for entry in archive.entries()? {
        let entry = entry?;
        let name = entry.path()?;
        let name = name.to_str().ok_or_else(|| invalid("non-UTF-8 path"))?;
        let name = name.trim_end_matches('/');
        let parts: Vec<_> = name.split('/').collect();
        if parts.first() != Some(&"ContextTrace.app")
            || parts.iter().any(|p| {
                p.is_empty() || *p == "." || *p == ".." || p.contains('\\') || p.starts_with("._")
            })
        {
            return Err(invalid("unsafe path, metadata, or unexpected archive root"));
        }
        let kind = entry.header().entry_type();
        if !kind.is_dir() && !kind.is_file() && !kind.is_symlink() {
            return Err(invalid("hard links and special entries are not allowed"));
        }
        if parts.len() == 1 && !kind.is_dir() {
            return Err(invalid("archive root must be a directory"));
        }
        if !seen.insert(name.to_owned()) {
            return Err(invalid("duplicate archive entry"));
        }
        if kind.is_symlink() {
            let target = entry
                .link_name()?
                .ok_or_else(|| invalid("missing symlink target"))?;
            let target = target
                .to_str()
                .ok_or_else(|| invalid("non-UTF-8 symlink target"))?;
            if target.starts_with('/') || target.contains('\\') || target.is_empty() {
                return Err(invalid("unsafe symlink target"));
            }
            links.insert(name.to_owned(), target.to_owned());
        }
        found_file |= kind.is_file();
    }
    if !found_file {
        return Err(invalid("archive has no app files"));
    }
    // No archived entry may write through an earlier symlink, regardless of
    // ordering. Symlinks may point to other links, as in macOS frameworks.
    for name in &seen {
        let parts: Vec<_> = name.split('/').collect();
        for n in 1..parts.len() {
            if links.contains_key(&parts[..n].join("/")) {
                return Err(invalid("archive entry traverses a symlink"));
            }
        }
    }
    for (name, target) in &links {
        let mut stack: Vec<String> = name.split('/').map(str::to_owned).collect();
        stack.pop();
        let mut pending: VecDeque<String> = target.split('/').map(str::to_owned).collect();
        let mut hops = 0;
        while let Some(part) = pending.pop_front() {
            match part.as_str() {
                "" | "." => continue,
                ".." => {
                    if stack.len() <= 1 {
                        return Err(invalid("symlink escapes app"));
                    }
                    stack.pop();
                }
                _ => {
                    stack.push(part);
                    if let Some(next) = links.get(&stack.join("/")) {
                        hops += 1;
                        if hops > 40 {
                            return Err(invalid("symlink cycle"));
                        }
                        stack.pop();
                        for component in next.split('/').rev() {
                            pending.push_front(component.to_owned());
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn extract(path: &Path, dest: &Path) -> io::Result<()> {
    validate(path)?;
    // A new empty destination ensures no pre-existing symlinks can redirect writes.
    fs::create_dir(dest)?;
    let mut archive = tar::Archive::new(GzDecoder::new(fs::File::open(path)?));
    for entry in archive.entries()? {
        let mut entry = entry?;
        // Match tauri-plugin-updater's macOS extraction loop: strip the first
        // path component, create parents, then call Entry::unpack.
        let collected: std::path::PathBuf = entry.path()?.iter().skip(1).collect();
        let destination = dest.join(collected);
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)?;
        }
        entry.unpack(&destination)?;
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 {
        return Err(invalid(
            "usage: verify-macos-updater-archive ARCHIVE NEW_DESTINATION",
        ));
    }
    extract(Path::new(&args[1]), Path::new(&args[2]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{write::GzEncoder, Compression};

    fn fixture(
        entries: &[(&str, tar::EntryType, &str)],
    ) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.tar.gz");
        let gzip = GzEncoder::new(fs::File::create(&path).unwrap(), Compression::default());
        let mut tar = tar::Builder::new(gzip);
        for (name, kind, target) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_entry_type(*kind);
            header.set_mode(0o755);
            header.set_size(if kind.is_file() { 1 } else { 0 });
            if kind.is_symlink() || kind.is_hard_link() {
                header.set_link_name(target).unwrap();
            }
            // Write raw names so traversal fixtures aren't sanitized by the builder.
            header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
            header.set_cksum();
            tar.append(&header, if kind.is_file() { &b"x"[..] } else { &b""[..] })
                .unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
        (dir, path)
    }

    #[test]
    fn valid_app_extracts_like_updater() {
        let (dir, path) = fixture(&[
            ("ContextTrace.app/", tar::EntryType::Directory, ""),
            ("ContextTrace.app/Contents/", tar::EntryType::Directory, ""),
            (
                "ContextTrace.app/Contents/Info.plist",
                tar::EntryType::Regular,
                "",
            ),
        ]);
        let dest = dir.path().join("extracted");
        extract(&path, &dest).unwrap();
        assert_eq!(fs::read(dest.join("Contents/Info.plist")).unwrap(), b"x");
    }

    #[test]
    fn rejects_extra_roots_metadata_traversal_and_links() {
        for name in [
            "._ContextTrace.app",
            "other/file",
            "ContextTrace.app/../escape",
            "ContextTrace.app/Contents/._Info.plist",
        ] {
            let (_dir, path) = fixture(&[(name, tar::EntryType::Regular, "")]);
            assert!(validate(&path).is_err(), "accepted {name}");
        }
        let (_dir, path) = fixture(&[(
            "ContextTrace.app/link",
            tar::EntryType::Symlink,
            "../outside",
        )]);
        assert!(validate(&path).is_err());
    }

    #[test]
    fn permits_internal_framework_links_but_rejects_link_traversal() {
        let (_dir, path) = fixture(&[
            (
                "ContextTrace.app/Contents/Frameworks/F/Versions/A/F",
                tar::EntryType::Regular,
                "",
            ),
            (
                "ContextTrace.app/Contents/Frameworks/F/Versions/Current",
                tar::EntryType::Symlink,
                "A",
            ),
            (
                "ContextTrace.app/Contents/Frameworks/F/F",
                tar::EntryType::Symlink,
                "Versions/Current/F",
            ),
        ]);
        assert!(validate(&path).is_ok());
        let (_dir, path) = fixture(&[
            (
                "ContextTrace.app/Contents/link",
                tar::EntryType::Symlink,
                "real",
            ),
            (
                "ContextTrace.app/Contents/link/file",
                tar::EntryType::Regular,
                "",
            ),
        ]);
        assert!(validate(&path).is_err());
        let (_dir, path) = fixture(&[
            (
                "ContextTrace.app/Contents/file",
                tar::EntryType::Regular,
                "",
            ),
            (
                "ContextTrace.app/Contents/hard",
                tar::EntryType::Link,
                "Contents/file",
            ),
        ]);
        assert!(validate(&path).is_err());
        let (_dir, path) = fixture(&[
            (
                "ContextTrace.app/Contents/file",
                tar::EntryType::Regular,
                "",
            ),
            ("ContextTrace.app/Contents/a", tar::EntryType::Symlink, "b"),
            ("ContextTrace.app/Contents/b", tar::EntryType::Symlink, "a"),
        ]);
        assert!(validate(&path).is_err());
    }
}
