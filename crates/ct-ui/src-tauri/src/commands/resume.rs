//! Native resume uses a catalog identity, never caller-supplied command text.
use super::{AppState, SessionDescriptor, SessionFilter, ThreadRole};
use base64::{engine::general_purpose::STANDARD, Engine};
use ct_domain::AgentKind;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResumePlan {
    pub agent: String,
    pub id: String,
    pub project: Option<String>,
    pub directory: Option<String>,
    pub command: String,
    pub can_launch: bool,
    pub reason: Option<String>,
    pub is_parent: bool,
}

fn uuid(id: &str) -> bool {
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if [8, 13, 18, 23].contains(&index) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn target<'a>(
    descriptors: &'a [SessionDescriptor],
    id: &str,
) -> Result<&'a SessionDescriptor, String> {
    let mut current = id;
    let mut visited = std::collections::HashSet::new();
    loop {
        if !visited.insert(current) {
            return Err("This session has a circular parent reference.".into());
        }
        let descriptor = descriptors.iter().find(|descriptor| descriptor.id.as_str() == current)
            .ok_or("Native resume needs the original agent session log. The session or its parent is unavailable; an archived copy alone cannot be resumed.")?;
        match &descriptor.thread_role {
            ThreadRole::Root => return Ok(descriptor),
            ThreadRole::Subagent { parent } => current = parent.as_str(),
        }
    }
}

fn program(agent: AgentKind) -> &'static str {
    match agent {
        AgentKind::Codex => "codex",
        AgentKind::ClaudeCode => "claude",
    }
}

fn executable(agent: AgentKind) -> Option<PathBuf> {
    let name = program(agent);
    let mut directories: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .filter(|path| path.is_absolute())
                .collect()
        })
        .unwrap_or_default();
    if let Some(home) = std::env::var_os("USERPROFILE") {
        directories.push(PathBuf::from(home).join(".local/bin"));
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        directories.push(PathBuf::from(local).join("Programs/OpenAI/Codex/bin"));
    }
    let extensions: &[&str] = if cfg!(windows) {
        &["exe", "cmd", "ps1"]
    } else {
        &[""]
    };
    directories
        .into_iter()
        .flat_map(|directory| {
            extensions.iter().map(move |extension| {
                directory.join(if extension.is_empty() {
                    name.to_string()
                } else {
                    format!("{name}.{extension}")
                })
            })
        })
        .find(|path| path.is_file())
}

// PowerShell recognises typographic quotes as delimiters too. Keep those and
// all shell metacharacters out of the executable script's data representation.
fn encoded_string(value: &str) -> String {
    format!(
        "([System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('{}')))",
        STANDARD.encode(value)
    )
}

fn display_string(value: &str) -> String {
    if value.contains(['\u{2018}', '\u{2019}', '\u{201c}', '\u{201d}']) {
        encoded_string(value)
    } else {
        format!("'{}'", value.replace('\'', "''"))
    }
}

fn arguments(agent: AgentKind, id: &str) -> String {
    match agent {
        AgentKind::Codex => format!("resume '{id}'"),
        AgentKind::ClaudeCode => format!("--resume '{id}'"),
    }
}

fn plan(
    descriptor: &SessionDescriptor,
    requested_id: &str,
    directory: Option<String>,
    executable: Option<&Path>,
) -> Result<ResumePlan, String> {
    let id = descriptor.id.as_str();
    if !uuid(id) {
        return Err("This log does not record a resumable session UUID.".into());
    }
    let directory = directory.or_else(|| descriptor.project.clone());
    let reason = match directory.as_deref() {
        None => Some("No working folder was recorded. Enter an existing local folder.".to_string()),
        Some(path) if !super::local_files::local_absolute(path) => Some("Enter an absolute local working folder.".to_string()),
        Some(path) if !Path::new(path).is_dir() => Some("The working folder is missing or inaccessible. Enter an existing folder.".to_string()),
        _ if !cfg!(windows) => Some("Opening a resume terminal is currently supported on Windows. Copy the command instead.".to_string()),
        _ if executable.is_none() => Some(format!("{} was not found. Install its CLI or add it to PATH, then retry. You can also copy the command.", program(descriptor.agent))),
        _ => None,
    };
    let executable = executable
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| program(descriptor.agent).into());
    let mut invocation = format!(
        "& {} {}",
        display_string(&executable),
        arguments(descriptor.agent, id)
    );
    if descriptor.agent == AgentKind::Codex {
        if let Some(path) = directory.as_deref() {
            invocation.push_str(&format!(" --cd {}", display_string(path)));
        }
    }
    let command = match directory.as_deref() {
        Some(path) => format!(
            "Set-Location -LiteralPath {} -ErrorAction Stop; {invocation}",
            display_string(path)
        ),
        None => invocation,
    };
    Ok(ResumePlan {
        agent: descriptor.agent.to_string(),
        id: id.into(),
        project: descriptor.project.clone(),
        directory,
        command,
        can_launch: reason.is_none(),
        reason,
        is_parent: id != requested_id,
    })
}

fn prepare(
    state: &AppState,
    agent: &str,
    id: &str,
    directory: Option<String>,
) -> Result<(ResumePlan, Option<PathBuf>), String> {
    let agent = super::parse_agent(agent)?;
    // Deliberately live logs only: an archive is evidence, not a session we can
    // silently restore into another application's private storage.
    let descriptors = state.app.list_sessions(&SessionFilter {
        agent: Some(agent),
        ..Default::default()
    });
    let descriptor = target(&descriptors, id)?;
    let executable = executable(agent);
    Ok((
        plan(descriptor, id, directory, executable.as_deref())?,
        executable,
    ))
}

#[tauri::command]
pub fn prepare_resume(
    state: tauri::State<'_, AppState>,
    agent: String,
    id: String,
    directory: Option<String>,
) -> Result<ResumePlan, String> {
    prepare(&state, &agent, &id, directory).map(|(plan, _)| plan)
}

#[cfg(windows)]
fn terminal_command(plan: &ResumePlan, executable: &Path) -> Result<std::process::Command, String> {
    use std::os::windows::process::CommandExt;
    let directory = plan
        .directory
        .as_deref()
        .ok_or("Choose a working folder first.")?;
    let agent = super::parse_agent(&plan.agent)?;
    let mut script = format!(
        "$ErrorActionPreference = 'Stop'; Set-Location -LiteralPath {}; & {} {}",
        encoded_string(directory),
        encoded_string(&executable.display().to_string()),
        arguments(agent, &plan.id)
    );
    if agent == AgentKind::Codex {
        script.push_str(&format!(" --cd {}", encoded_string(directory)));
    }
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let windows =
        std::env::var_os("SystemRoot").ok_or("Windows system directory is unavailable.")?;
    let mut command = std::process::Command::new(
        PathBuf::from(windows).join("System32/WindowsPowerShell/v1.0/powershell.exe"),
    );
    command
        .args([
            "-NoLogo",
            "-NoProfile",
            "-NoExit",
            "-EncodedCommand",
            &STANDARD.encode(bytes),
        ])
        // The user explicitly requested an interactive terminal. Other desktop
        // subprocesses remain hidden; this one needs its own visible console.
        .creation_flags(0x0000_0010);
    Ok(command)
}

#[cfg(windows)]
fn launch(plan: &ResumePlan, executable: &Path) -> Result<(), String> {
    terminal_command(plan, executable)?
        .spawn()
        .map_err(|error| format!("Could not open the resume terminal: {error}"))?;
    Ok(())
}

#[cfg(not(windows))]
fn launch(_plan: &ResumePlan, _executable: &Path) -> Result<(), String> {
    Err("Opening a resume terminal is currently supported on Windows.".into())
}

#[tauri::command]
pub fn resume_session(
    state: tauri::State<'_, AppState>,
    agent: String,
    id: String,
    directory: Option<String>,
) -> Result<(), String> {
    // Re-resolve everything on click. The preview is not a capability to run a
    // stale command after the source log, folder or CLI has disappeared.
    let (plan, executable) = prepare(&state, &agent, &id, directory)?;
    if let Some(reason) = plan.reason {
        return Err(reason);
    }
    launch(
        &plan,
        executable
            .as_deref()
            .ok_or("The agent CLI is unavailable.")?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn powershell_launcher_passes_exact_ids_and_directories_to_a_stub_cli() {
        use std::os::windows::process::CommandExt;
        let directory = std::env::temp_dir().join(format!(
            "ct-resume-{} O'Brien; $safe \u{2019}",
            std::process::id()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let executable = directory.join("stub agent.cmd");
        // A fixture, not an installed agent: no network, session writes, or
        // interactive console. Exercise the production encoded launch command.
        std::fs::write(&executable, "@echo off\r\necho %*\r\ncd\r\n").unwrap();
        let mut descriptor = super::super::tests::catalog_descriptor(0, "unused");
        descriptor.id = ct_domain::SessionId::new("12345678-1234-1234-1234-123456789abc").unwrap();
        for agent in [AgentKind::Codex, AgentKind::ClaudeCode] {
            descriptor.agent = agent;
            let plan = plan(
                &descriptor,
                descriptor.id.as_str(),
                Some(directory.display().to_string()),
                Some(&executable),
            )
            .unwrap();
            assert!(plan.can_launch);
            let command = terminal_command(&plan, &executable).unwrap();
            let args: Vec<_> = command.get_args().filter(|arg| *arg != "-NoExit").collect();
            let output = std::process::Command::new(command.get_program())
                .args(args)
                .creation_flags(0x0800_0000)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let text = String::from_utf8_lossy(&output.stdout);
            assert!(text.contains(descriptor.id.as_str()), "{text}");
            assert!(
                text.contains(if agent == AgentKind::Codex {
                    "resume"
                } else {
                    "--resume"
                }),
                "{text}"
            );
            assert!(
                text.contains("--cd") == (agent == AgentKind::Codex),
                "{text}"
            );
            assert!(text.contains("O'Brien; $safe"), "{text}");
        }
        std::fs::remove_file(executable).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn command_and_directory_are_quoted_without_executing_log_text() {
        let mut descriptor = super::super::tests::catalog_descriptor(0, "unused");
        descriptor.id = ct_domain::SessionId::new("12345678-1234-1234-1234-123456789abc").unwrap();
        let directory = "C:/work/O'Brien; $(bad) & more";
        let codex = plan(
            &descriptor,
            descriptor.id.as_str(),
            Some(directory.into()),
            None,
        )
        .unwrap();
        assert!(codex.command.contains("'C:/work/O''Brien; $(bad) & more'"));
        assert!(codex
            .command
            .contains("resume '12345678-1234-1234-1234-123456789abc'"));
        descriptor.agent = AgentKind::ClaudeCode;
        assert!(plan(&descriptor, "child", Some(directory.into()), None)
            .unwrap()
            .command
            .contains("--resume '12345678"));
        assert!(!encoded_string(directory).contains("$(bad)"));
        assert!(!display_string("C:/smart\u{2019}; bad").contains("; bad"));
    }

    #[test]
    fn unrecorded_missing_and_non_uuid_sessions_cannot_launch() {
        let mut descriptor = super::super::tests::catalog_descriptor(0, "missing");
        assert!(plan(&descriptor, "0", None, None).is_err());
        descriptor.id = ct_domain::SessionId::new("12345678-1234-1234-1234-123456789abc").unwrap();
        descriptor.project = None;
        assert!(
            !plan(&descriptor, descriptor.id.as_str(), None, None)
                .unwrap()
                .can_launch
        );
        let missing = std::env::temp_dir().join("ct-resume-no-such-directory");
        let plan = plan(
            &descriptor,
            descriptor.id.as_str(),
            Some(missing.display().to_string()),
            None,
        )
        .unwrap();
        assert!(plan.reason.unwrap().contains("missing"));
    }

    #[test]
    fn subagents_resolve_to_their_parent_and_cycles_or_missing_parents_fail() {
        let mut parent = super::super::tests::catalog_descriptor(0, "project");
        parent.id = ct_domain::SessionId::new("parent").unwrap();
        let mut child = parent.clone();
        child.id = ct_domain::SessionId::new("child").unwrap();
        child.thread_role = ThreadRole::Subagent {
            parent: parent.id.clone(),
        };
        assert_eq!(
            target(&[parent, child.clone()], "child")
                .unwrap()
                .id
                .as_str(),
            "parent"
        );
        assert!(target(&[child.clone()], "child").is_err());
        child.thread_role = ThreadRole::Subagent {
            parent: child.id.clone(),
        };
        assert!(target(&[child], "child").unwrap_err().contains("circular"));
    }
}
