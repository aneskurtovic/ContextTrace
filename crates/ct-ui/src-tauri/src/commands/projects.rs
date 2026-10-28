//! Folder classification is presentation metadata; it never removes sessions.
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DirectoryState {
    Available,
    Missing,
    Unknown,
}

pub fn directory_state(path: Option<&str>) -> DirectoryState {
    let Some(path) = path.filter(|path| super::local_files::local_absolute(path)) else {
        return DirectoryState::Unknown;
    };
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => DirectoryState::Available,
        Ok(_) => DirectoryState::Missing,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => DirectoryState::Missing,
        Err(_) => DirectoryState::Unknown,
    }
}

fn normalized(path: &str) -> String {
    let path = path.replace('\\', "/");
    let windows = path.as_bytes().get(1) == Some(&b':');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "." => {}
            ".." => {
                parts.pop();
            }
            _ => parts.push(part),
        }
    }
    let result = parts.join("/").trim_end_matches('/').to_string();
    if windows {
        result.to_ascii_lowercase()
    } else {
        result
    }
}

fn within(path: &str, root: &str) -> bool {
    !root.is_empty()
        && (path == root
            || path
                .strip_prefix(root)
                .is_some_and(|tail| tail.starts_with('/')))
}

pub fn is_temporary(path: &str) -> bool {
    is_temporary_with_root(path, &std::env::temp_dir().to_string_lossy())
}

fn is_temporary_with_root(path: &str, configured: &str) -> bool {
    let path = normalized(path);
    let configured = normalized(configured);
    if within(&path, &configured)
        || ["/tmp", "/var/tmp", "/private/tmp", "/private/var/tmp"]
            .iter()
            .any(|root| within(&path, root))
    {
        return true;
    }
    // Historical Windows logs can belong to another user or drive. Recognise
    // the OS location, never an arbitrary repository directory named "tmp".
    let parts: Vec<_> = path.split('/').collect();
    let drive = parts.first().is_some_and(|part| {
        part.len() == 2 && part.as_bytes()[0].is_ascii_alphabetic() && part.ends_with(':')
    });
    drive
        && ((parts.len() >= 3 && parts[1..3] == ["windows", "temp"])
            || (parts.len() >= 6
                && parts[1] == "users"
                && !parts[2].is_empty()
                && parts[3..6] == ["appdata", "local", "temp"]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_locations_use_path_boundaries_and_survive_directory_deletion() {
        for path in [
            r"C:\Users\me\AppData\Local\Temp\.tmp123",
            "c:/WINDOWS/TEMP/task",
            "/tmp/gone",
            "/private/var/tmp/task",
            "/custom/scratch/task",
        ] {
            assert!(is_temporary_with_root(path, "/custom/scratch"), "{path}");
        }
        for path in [
            "C:/repos/tmp-project",
            "C:/repos/Temp/task",
            "/tmp-project",
            "/tmp/../repos/project",
            "/custom/scratchpad",
            "/repo/tmp",
        ] {
            assert!(!is_temporary_with_root(path, "/custom/scratch"), "{path}");
        }
    }

    #[test]
    fn missing_is_distinct_from_unrecorded_and_available() {
        assert_eq!(directory_state(None), DirectoryState::Unknown);
        assert_eq!(
            directory_state(Some("legacy-slug")),
            DirectoryState::Unknown
        );
        let root = std::env::temp_dir();
        assert_eq!(directory_state(root.to_str()), DirectoryState::Available);
        let missing = root.join(format!("ct-no-directory-{}", std::process::id()));
        assert_eq!(directory_state(missing.to_str()), DirectoryState::Missing);
    }
}
