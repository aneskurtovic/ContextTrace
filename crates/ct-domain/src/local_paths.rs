//! Pure path policy shared by evidence-derived filesystem actions.
//! Refuse foreign/network/device/drive-relative names before any filesystem I/O.
use std::path::{Path, PathBuf};

pub fn local_absolute(path: &str) -> bool {
    if path.is_empty() || path.chars().any(char::is_control) || path.contains("://") {
        return false;
    }
    #[cfg(windows)]
    {
        let bytes = path.as_bytes();
        bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\')
            && !path[2..].contains(':')
            && safe_components(&path[3..])
    }
    #[cfg(not(windows))]
    {
        path.starts_with('/')
            && !path.starts_with("//")
            && !path.contains(':')
            && !path.contains('\\')
    }
}

fn safe_components(path: &str) -> bool {
    path.split(['/', '\\']).all(|part| {
        if matches!(part, "" | "." | "..") {
            return true;
        }
        if part.ends_with(['.', ' ']) {
            return false;
        }
        let base = part
            .split('.')
            .next()
            .unwrap_or("")
            .trim_end_matches(' ')
            .to_ascii_uppercase();
        !matches!(
            base.as_str(),
            "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
        ) && !((base.starts_with("COM") || base.starts_with("LPT"))
            && base.len() == 4
            && matches!(base.as_bytes()[3], b'1'..=b'9'))
            && !["COM¹", "COM²", "COM³", "LPT¹", "LPT²", "LPT³"].contains(&base.as_str())
    })
}

/// Relative names require an explicitly recorded local absolute working directory.
pub fn resolve_local(path: &str, root: Option<&str>) -> Option<PathBuf> {
    if local_absolute(path) {
        return Some(PathBuf::from(path));
    }
    if path.is_empty()
        || path.contains(':')
        || path.starts_with(['/', '\\'])
        || path.chars().any(char::is_control)
        || !safe_components(path)
    {
        return None;
    }
    let root = root.filter(|root| local_absolute(root))?;
    let joined = Path::new(root).join(path);
    local_absolute(&joined.to_string_lossy()).then_some(joined)
}

/// canonicalize adds an extended-length prefix for local Windows drive paths.
pub fn canonical_local_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(local) = text.strip_prefix(r"\\?\") {
        if local.as_bytes().get(1) == Some(&b':') {
            return PathBuf::from(local);
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn foreign_paths_are_refused_without_io() {
        for bad in [
            r"\\server\share\AGENTS.md",
            "//server/share/file",
            r"\\?\C:\file",
            r"\\.\pipe\input",
            "C:relative",
            "C:/file:stream",
            "https://host/file",
            "file:///tmp/file",
            "AGENTS.md\n",
            "NUL",
            "con.txt",
            "COM1.md",
            "COM¹.md",
            "CON .txt",
            "file.",
        ] {
            assert!(
                resolve_local(bad, Some(if cfg!(windows) { "C:/repo" } else { "/repo" })).is_none(),
                "{bad}"
            );
        }
        assert!(resolve_local("AGENTS.md", None).is_none());
        assert!(resolve_local("AGENTS.md", Some(r"\\server\share")).is_none());
    }
    #[test]
    fn local_repository_and_home_paths_remain_supported() {
        let root = if cfg!(windows) { "C:/repo" } else { "/repo" };
        assert_eq!(
            resolve_local("AGENTS.md", Some(root)),
            Some(Path::new(root).join("AGENTS.md"))
        );
        let home = if cfg!(windows) {
            "C:/Users/user/CLAUDE.md"
        } else {
            "/home/user/CLAUDE.md"
        };
        assert_eq!(resolve_local(home, None), Some(PathBuf::from(home)));
    }
}
