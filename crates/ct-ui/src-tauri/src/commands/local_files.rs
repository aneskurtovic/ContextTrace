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

pub(super) use ct_domain::local_paths::local_absolute;

fn resolve(target: &FileTarget) -> Option<PathBuf> {
    ct_domain::local_paths::resolve_local(&target.path, target.working_directory.as_deref())
}

/// Only familiar passive data formats get an Open action. Active and unknown
/// associations remain Reveal-only, regardless of their recorded display name.
fn supported_data_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            [
                "txt", "md", "json", "jsonl", "csv", "tsv", "log", "yaml", "yml", "toml", "xml",
                "ini", "cfg", "conf", "png", "jpg", "jpeg", "gif", "bmp", "webp", "ico", "tif",
                "tiff", "pdf", "mp3", "wav", "flac", "ogg", "mp4", "webm", "mov", "avi",
            ]
            .iter()
            .any(|candidate| ext.eq_ignore_ascii_case(candidate))
        })
}

fn can_open_file(recorded: &Path, canonical: &Path) -> bool {
    supported_data_file(recorded) && supported_data_file(canonical)
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
            let canonical = ct_domain::local_paths::canonical_local_path(&canonical);
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
            dto.resolved_path = Some(canonical.display().to_string());
            dto.can_open =
                metadata.is_dir() || (metadata.is_file() && can_open_file(&path, &canonical));
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
        FileAction::Open => return Err("Use Show in Explorer for this file type.".into()),
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
        assert!(!supported_data_file(Path::new("RUN.CMD")));
        assert!(supported_data_file(Path::new("screenshot.png")));
    }
    #[test]
    fn active_unknown_and_disguised_targets_are_reveal_only() {
        for name in [
            "payload.cpl",
            "payload.CPL",
            "payload.exe",
            "payload.dll",
            "payload.msc",
            "payload.lnk",
            "payload.url",
            "payload.hta",
            "payload.js",
            "payload.ps1",
            "payload.docm",
            "payload.html",
            "payload.svg",
            "payload",
            "payload.unknown",
        ] {
            assert!(!can_open_file(Path::new(name), Path::new(name)), "{name}");
        }
        // A data-looking link cannot make its active canonical target openable.
        assert!(!can_open_file(
            Path::new("photo.png"),
            Path::new("payload.cpl")
        ));
        assert!(!can_open_file(
            Path::new("payload.cpl"),
            Path::new("photo.png")
        ));
        assert!(can_open_file(
            Path::new("photo.PNG"),
            Path::new("photo.png")
        ));
    }

    #[test]
    fn existing_cpl_is_available_for_reveal_without_open() {
        let dir = std::env::temp_dir().join(format!("ct-file-policy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("fixture.CPL");
        std::fs::write(&file, b"inert data; never dispatched").unwrap();
        let info = inspect(&FileTarget {
            path: file.display().to_string(),
            working_directory: None,
        });
        assert_eq!(info.status, "file");
        assert!(!info.can_open);
        assert!(info.resolved_path.is_some());
        std::fs::remove_file(file).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
