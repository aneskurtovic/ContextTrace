//! Minimal recursive directory walking.
//!
//! Replaces `walkdir`, which is a fine crate but drags in `windows-sys` for
//! functionality this project does not use. Two reasons that trade is worth
//! making here:
//!
//! - **Build portability.** `windows-sys` requires mingw's `dlltool` on the
//!   windows-gnu toolchain, which is not on `PATH` in a default install.
//! - **Audit surface.** ContextTrace reads session logs containing source code,
//!   prompts and potentially secrets. The fewer third-party crates in the tree,
//!   the cheaper it is to verify the "nothing leaves this machine" claim.

use std::fs;
use std::path::{Path, PathBuf};

/// Depth cap. Session directories nest a few levels at most (Codex uses
/// `sessions/YYYY/MM/DD`); anything deeper is a symlink cycle or a mistake, and
/// bounding it is cheaper than tracking visited inodes.
const MAX_DEPTH: usize = 16;

/// Collect files under `root` for which `keep` returns true.
///
/// Unreadable directories are skipped rather than propagated: one permission
/// error in a corner of the tree must not deny the user every session found
/// elsewhere.
pub fn find_files(root: &Path, keep: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, 0, &keep, &mut out);
    out
}

fn walk(dir: &Path, depth: usize, keep: &impl Fn(&Path) -> bool, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        // `file_type` avoids a second stat syscall per entry, which matters on
        // a corpus of several hundred files.
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        if file_type.is_dir() {
            walk(&path, depth + 1, keep, out);
        } else if file_type.is_file() && keep(&path) {
            out.push(path);
        }
        // Symlinks are deliberately not followed: a session directory should
        // not be able to redirect us outside the roots we told the user about.
    }
}

/// True when `path` ends in the given extension, case-insensitively.
pub fn has_extension(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ct-walk-{name}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("2026").join("07")).unwrap();
        File::create(dir.join("top.jsonl")).unwrap();
        File::create(dir.join("2026").join("mid.jsonl")).unwrap();
        File::create(dir.join("2026").join("07").join("deep.jsonl")).unwrap();
        File::create(dir.join("2026").join("notes.txt")).unwrap();
        dir
    }

    #[test]
    fn finds_files_at_every_depth() {
        let dir = scratch("depth");
        let mut found: Vec<String> = find_files(&dir, |p| has_extension(p, "jsonl"))
            .iter()
            .filter_map(|p| p.file_name()?.to_str().map(String::from))
            .collect();
        found.sort();
        assert_eq!(found, vec!["deep.jsonl", "mid.jsonl", "top.jsonl"]);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn filters_by_predicate() {
        let dir = scratch("filter");
        let txt = find_files(&dir, |p| has_extension(p, "txt"));
        assert_eq!(txt.len(), 1);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_root_yields_nothing_rather_than_panicking() {
        assert!(find_files(Path::new("Z:/no/such/place"), |_| true).is_empty());
    }

    #[test]
    fn extension_matching_ignores_case() {
        assert!(has_extension(Path::new("a/b.JSONL"), "jsonl"));
        assert!(!has_extension(Path::new("a/b.json"), "jsonl"));
        assert!(!has_extension(Path::new("a/b"), "jsonl"));
    }
}
