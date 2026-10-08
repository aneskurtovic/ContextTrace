//! File actions are derived from parsed session evidence, never display text or
//! a caller-provided path. Every action checks the filesystem again.
use super::AppState;
use ct_domain::model::event::{EventKind, FileTarget};
use ct_domain::{AgentSession, ContextItem, ContextSource};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri_plugin_opener::OpenerExt;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTargetDto {
    pub path: String,
    pub resolved_path: Option<String>,
    pub status: &'static str,
    pub can_open: bool,
}

pub struct TargetIndex<'a> {
    events: HashMap<u32, &'a EventKind>,
    calls: HashMap<&'a str, &'a FileTarget>,
}

impl<'a> TargetIndex<'a> {
    pub fn new(session: &'a AgentSession, items: &[ContextItem]) -> Self {
        let mut events = HashMap::new();
        let mut calls = HashMap::new();
        let present: std::collections::HashSet<_> = items
            .iter()
            .filter_map(|item| item.provenance.source.map(|source| source.line_no))
            .collect();
        for event in session.events() {
            events.insert(event.source.line_no, &event.kind);
            // Claude logs contain abandoned sibling branches. A call from one
            // must not supply the file action for a result on the selected chain.
            if !present.contains(&event.source.line_no) {
                continue;
            }
            if let EventKind::ToolCall {
                call_id: Some(id),
                file_target: Some(target),
                ..
            } = &event.kind
            {
                calls.insert(id.as_str(), target);
            }
        }
        Self { events, calls }
    }

    pub fn target(&self, item: &ContextItem) -> Option<FileTargetDto> {
        let direct = match &item.source {
            ContextSource::FileRead { path }
            | ContextSource::InstructionFile { path }
            | ContextSource::ProjectConfig { path: Some(path) } => Some(FileTarget {
                path: path.clone(),
                working_directory: None,
            }),
            _ => None,
        };
        let target = direct.as_ref().or_else(|| {
            let event = self.events.get(&item.provenance.source?.line_no)?;
            match event {
                EventKind::ToolCall { file_target, .. } => file_target.as_ref(),
                EventKind::ToolResult {
                    call_id: Some(id), ..
                }
                | EventKind::OversizedToolResult {
                    call_id: Some(id), ..
                } => self.calls.get(id.as_str()).copied(),
                _ => None,
            }
        })?;
        Some(inspect(target))
    }
}

fn local_absolute(path: &str) -> bool {
    if path.chars().any(char::is_control) || path.contains("://") {
        return false;
    }
    #[cfg(windows)]
    {
        // Exclude UNC/device paths, drive-relative names and alternate streams.
        let bytes = path.as_bytes();
        bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\')
            && !path[2..].contains(':')
    }
    #[cfg(not(windows))]
    {
        path.starts_with('/') && !path.starts_with("//")
    }
}

fn resolve(target: &FileTarget) -> Option<PathBuf> {
    if local_absolute(&target.path) {
        return Some(PathBuf::from(&target.path));
    }
    let path = Path::new(&target.path);
    if path.is_absolute()
        || target.path.contains(':')
        || target.path.starts_with(['/', '\\'])
        || target.path.chars().any(char::is_control)
        || target.path.is_empty()
    {
        return None;
    }
    let cwd = target.working_directory.as_deref()?;
    local_absolute(cwd).then(|| Path::new(cwd).join(path))
}

fn executable(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            [
                "exe", "com", "bat", "cmd", "ps1", "msi", "msp", "lnk", "url", "hta", "reg", "vbs",
                "vbe", "js", "jse", "wsf", "wsh", "scr", "pif",
            ]
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

fn inspect(target: &FileTarget) -> FileTargetDto {
    let mut dto = FileTargetDto {
        path: target.path.clone(),
        resolved_path: None,
        status: "unresolved",
        can_open: false,
    };
    let Some(path) = resolve(target) else {
        return dto;
    };
    dto.resolved_path = Some(path.display().to_string());
    match std::fs::metadata(&path) {
        Ok(metadata) => {
            // A link may point at a network/device location or an executable.
            let Ok(canonical) = std::fs::canonicalize(&path) else {
                dto.status = "unreadable";
                return dto;
            };
            let canonical = dunce_path(&canonical);
            if !local_absolute(&canonical.display().to_string()) {
                return dto;
            }
            dto.status = if metadata.is_file() {
                "file"
            } else if metadata.is_dir() {
                "directory"
            } else {
                "unreadable"
            };
            dto.can_open = metadata.is_dir()
                || (metadata.is_file() && !executable(&path) && !executable(&canonical));
        }
        Err(error) => {
            dto.status = if error.kind() == std::io::ErrorKind::NotFound {
                "missing"
            } else {
                "unreadable"
            }
        }
    }
    dto
}

fn dunce_path(path: &Path) -> PathBuf {
    // Windows canonicalize adds the extended-length prefix to local drives.
    let text = path.to_string_lossy();
    if let Some(local) = text.strip_prefix(r"\\?\") {
        if local.as_bytes().get(1) == Some(&b':') {
            return PathBuf::from(local);
        }
    }
    path.to_path_buf()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FileAction {
    Open,
    Reveal,
}

#[tauri::command]
pub fn open_context_file(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    agent: String,
    id: String,
    turn: u32,
    item_id: String,
    action: FileAction,
) -> Result<(), String> {
    let agent = super::parse_agent(&agent)?;
    let context = state.context(agent, &id, Some(turn))?;
    let target = context
        .items
        .into_iter()
        .find(|item| item.id == item_id)
        .and_then(|item| item.file_target)
        .ok_or("This item has no recorded filesystem target.")?;
    if !matches!(target.status, "file" | "directory") {
        return Err(match target.status {
            "missing" => "File no longer available.",
            _ => "This path is not available locally.",
        }
        .into());
    }
    let path = target
        .resolved_path
        .ok_or("This path could not be resolved.")?;
    match action {
        FileAction::Open if target.can_open => app.opener().open_path(path, None::<&str>),
        FileAction::Open => return Err("Use Show in Explorer for executable files.".into()),
        FileAction::Reveal => app.opener().reveal_item_in_dir(path),
    }
    .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn relative_paths_need_the_calls_own_directory_and_foreign_paths_are_unresolved() {
        let mut target = FileTarget {
            path: "a  b.png".into(),
            working_directory: None,
        };
        assert!(resolve(&target).is_none());
        target.working_directory = Some(std::env::temp_dir().display().to_string());
        assert_eq!(resolve(&target).unwrap().file_name().unwrap(), "a  b.png");
        for path in [
            "https://example.com/a.png",
            r"C:relative.png",
            r"\\server\share\a.png",
            r"\\?\C:\a.png",
            "file:///tmp/a.png",
        ] {
            assert!(!local_absolute(path), "{path}");
        }
        #[cfg(windows)]
        assert!(!local_absolute("/tmp/a.png"));
    }
    #[test]
    fn missing_paths_and_executables_do_not_claim_to_open() {
        let path =
            std::env::temp_dir().join(format!("contexttrace-missing-{}.png", std::process::id()));
        let info = inspect(&FileTarget {
            path: path.display().to_string(),
            working_directory: None,
        });
        assert_eq!(info.status, "missing");
        assert!(!info.can_open);
        assert!(executable(Path::new("RUN.CMD")));
        assert!(!executable(Path::new("screenshot.png")));
    }
}
