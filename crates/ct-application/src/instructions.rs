//! Observed instruction-artifact signatures and drift.
//!
//! The normalized event model intentionally does not retain instruction text.
//! This report therefore compares the facts the harness recorded — mechanism,
//! label and character length — rather than pretending to compare content it
//! never loaded. That makes a change useful for triage while keeping the
//! report safe to expose to an agent.

use ct_domain::ports::{ContentHasher, InstructionFileReader, InstructionReadError};
use ct_domain::{AgentSession, EventKind};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstructionObservation {
    pub turn: Option<u32>,
    pub mechanism: String,
    pub label: String,
    pub char_len: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstructionChange {
    pub mechanism: String,
    pub from_turn: Option<u32>,
    pub to_turn: Option<u32>,
    pub from_label: String,
    pub to_label: String,
    pub from_char_len: u32,
    pub to_char_len: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InstructionDrift {
    pub session_id: String,
    pub base_instructions_observed: bool,
    pub observations: Vec<InstructionObservation>,
    pub changes: Vec<InstructionChange>,
    pub content_compared: bool,
}

/// Whether an observed instruction file can be compared with the bytes on
/// disk now. These are intentionally not a boolean: a missing file, an
/// unreadable file, and a session parsed without retaining the recorded body
/// require different user actions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstructionFileStatus {
    Matching,
    Changed,
    Missing,
    Unreadable,
    RecordedBodyUnavailable,
    UnsafePath,
    TooLarge,
    NotRegular,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionFileComparison {
    pub path: String,
    pub turn: Option<u32>,
    pub line: u32,
    pub status: InstructionFileStatus,
    pub recorded_digest: Option<String>,
    pub current_digest: Option<String>,
    pub recorded_chars: u32,
    pub current_chars: Option<u32>,
    pub comparison_basis: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct InstructionFileReport {
    pub session_id: String,
    pub project_root: Option<String>,
    pub comparisons: Vec<InstructionFileComparison>,
    pub refusal_count: usize,
}

/// Compare the recorded body fingerprint of each observed repository
/// instruction attachment with the current file. The session must have been
/// loaded with content analysis; ordinary parsing deliberately discards bodies
/// and therefore receives an explicit refusal instead of a size-only guess.
pub fn compare_files(
    session: &AgentSession,
    hasher: &dyn ContentHasher,
    reader: &dyn InstructionFileReader,
) -> InstructionFileReport {
    let project_root = session
        .metadata()
        .working_directory
        .clone()
        .or_else(|| session.metadata().project.clone());
    let mut comparisons = Vec::new();

    for event in session.events() {
        let EventKind::ContextInjection {
            mechanism,
            label,
            char_len,
        } = &event.kind
        else {
            continue;
        };
        if mechanism != "nested_memory" {
            continue;
        }

        let path = ct_domain::local_paths::resolve_local(label, project_root.as_deref());
        let recorded = event.content_measurement;
        let (status, current_digest, current_chars, detail) = match recorded {
            None => (
                InstructionFileStatus::RecordedBodyUnavailable,
                None,
                None,
                Some(
                    "the session was not loaded with content analysis; the recorded body is unavailable"
                        .into(),
                ),
            ),
            Some(recorded) => match path.as_ref()
                .ok_or(InstructionReadError::UnsafePath)
                .and_then(|path| reader.read(path)) {
                Ok(bytes) => {
                    let current = hasher.measure(&bytes);
                    let matching = current.fingerprint == recorded.fingerprint;
                    (
                        if matching {
                            InstructionFileStatus::Matching
                        } else {
                            InstructionFileStatus::Changed
                        },
                        Some(current.fingerprint.hex()),
                        Some(
                            String::from_utf8_lossy(&bytes)
                                .chars()
                                .count()
                                .min(u32::MAX as usize) as u32,
                        ),
                        Some(if matching {
                            "recorded attachment content fingerprint equals current file bytes"
                                .into()
                        } else {
                            "recorded attachment content fingerprint differs from current file bytes"
                                .into()
                        }),
                    )
                }
                Err(InstructionReadError::UnsafePath) => (
                    InstructionFileStatus::UnsafePath, None, None,
                    Some("only ordinary local paths on local drives may be compared".into()),
                ),
                Err(InstructionReadError::TooLarge) => (
                    InstructionFileStatus::TooLarge, None, None,
                    Some("instruction file exceeds the reader byte limit".into()),
                ),
                Err(InstructionReadError::NotRegular) => (
                    InstructionFileStatus::NotRegular, None, None,
                    Some("instruction target is not a regular file".into()),
                ),
                Err(InstructionReadError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => (
                    InstructionFileStatus::Missing,
                    None,
                    None,
                    Some(format!("file does not exist: {error}")),
                ),
                Err(error) => (
                    InstructionFileStatus::Unreadable,
                    None,
                    None,
                    Some(format!("file could not be read: {error}")),
                ),
            },
        };

        comparisons.push(InstructionFileComparison {
            path: path
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| label.clone()),
            turn: event.turn.map(|turn| turn.get()),
            line: event.source.line_no,
            status,
            recorded_digest: recorded.map(|value| value.fingerprint.hex()),
            current_digest,
            recorded_chars: *char_len,
            current_chars,
            comparison_basis:
                "SHA-256 of the recorded attachment payload versus SHA-256 of current file bytes"
                    .into(),
            detail,
        });
    }

    let refusal_count = comparisons
        .iter()
        .filter(|comparison| !matches!(comparison.status, InstructionFileStatus::Matching))
        .count();
    InstructionFileReport {
        session_id: session.id().to_string(),
        project_root,
        comparisons,
        refusal_count,
    }
}

pub fn inspect(session: &AgentSession) -> InstructionDrift {
    let mut observations = Vec::new();
    for event in session.events() {
        if let EventKind::ContextInjection {
            mechanism,
            label,
            char_len,
        } = &event.kind
        {
            observations.push(InstructionObservation {
                turn: event.turn.map(|turn| turn.get()),
                mechanism: mechanism.clone(),
                label: label.clone(),
                char_len: *char_len,
            });
        }
    }

    let mut previous: BTreeMap<(&str, &str), &InstructionObservation> = BTreeMap::new();
    let mut changes = Vec::new();
    for observation in &observations {
        // A prompt snapshot, file read, reminder or tool listing is not an
        // instruction revision. Compare repeated observations of the same
        // instruction artifact, not unrelated files sharing an attachment type.
        if !matches!(
            observation.mechanism.as_str(),
            "base_instructions" | "nested_memory" | "skill" | "dynamic_skill"
        ) {
            continue;
        }
        if let Some(prior) =
            previous.insert((&observation.mechanism, &observation.label), observation)
        {
            if prior.label != observation.label || prior.char_len != observation.char_len {
                changes.push(InstructionChange {
                    mechanism: observation.mechanism.clone(),
                    from_turn: prior.turn,
                    to_turn: observation.turn,
                    from_label: prior.label.clone(),
                    to_label: observation.label.clone(),
                    from_char_len: prior.char_len,
                    to_char_len: observation.char_len,
                });
            }
        }
    }

    InstructionDrift {
        session_id: session.id().to_string(),
        base_instructions_observed: session.metadata().base_instructions.is_some(),
        observations,
        changes,
        // We compare only recorded signatures, never instruction bodies.
        content_compared: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::event::EventLinks;
    use ct_domain::{
        AgentKind, ContentFingerprint, ContentHasher, ContentMeasurement, Event, EventId,
        EventKind, FileId, SessionId, SessionMetadata, SourceRef,
    };

    fn injection(label: &str, chars: u32) -> Event {
        Event {
            id: EventId::Ordinal(0),
            sequence: 0,
            timestamp: None,
            kind: EventKind::ContextInjection {
                mechanism: "skill".into(),
                label: label.into(),
                char_len: chars,
            },
            source: SourceRef::new(FileId(0), 0, 0, 1),
            raw_type: "context_injection".into(),
            turn: None,
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    #[test]
    fn reports_signature_change_without_claiming_content_comparison() {
        let session = AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            vec![injection("CLAUDE.md", 10), injection("CLAUDE.md", 12)],
            vec![],
            vec![],
        );
        let report = inspect(&session);
        assert_eq!(report.changes.len(), 1);
        assert!(!report.content_compared);
    }

    struct FixedHasher;

    #[test]
    fn snapshots_and_unrelated_instruction_files_do_not_imply_drift() {
        let mut events = vec![injection("CLAUDE.md", 10), injection("AGENTS.md", 20)];
        for event in &mut events {
            if let EventKind::ContextInjection { mechanism, .. } = &mut event.kind {
                *mechanism = "nested_memory".into();
            }
        }
        for chars in [100, 200, 300] {
            let mut event = injection("prompt snapshot", chars);
            if let EventKind::ContextInjection { mechanism, .. } = &mut event.kind {
                *mechanism = "prompt_snapshot".into();
            }
            events.push(event);
        }
        let session = AgentSession::new(
            SessionId::new("snapshots").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata::default(),
            events,
            vec![],
            vec![],
        );
        let report = inspect(&session);
        assert_eq!(report.observations.len(), 5, "keep attachment evidence");
        assert!(
            report.changes.is_empty(),
            "growing snapshots are not instructions"
        );
    }

    impl ContentHasher for FixedHasher {
        fn measure(&self, _bytes: &[u8]) -> ContentMeasurement {
            ContentMeasurement::new(ContentFingerprint::new([7; 32]), 12, 8)
        }
    }

    #[test]
    fn file_comparison_refuses_when_the_recorded_body_was_not_retained() {
        let mut event = injection("AGENTS.md", 10);
        event.kind = EventKind::ContextInjection {
            mechanism: "nested_memory".into(),
            label: "AGENTS.md".into(),
            char_len: 10,
        };
        let report = compare_files(
            &AgentSession::new(
                SessionId::new("s").unwrap(),
                AgentKind::ClaudeCode,
                SessionMetadata::default(),
                vec![event],
                vec![],
                vec![],
            ),
            &FixedHasher,
            &NeverReader,
        );
        assert_eq!(
            report.comparisons[0].status,
            InstructionFileStatus::RecordedBodyUnavailable
        );
    }
    struct NeverReader;
    impl InstructionFileReader for NeverReader {
        fn read(&self, _: &std::path::Path) -> Result<Vec<u8>, InstructionReadError> {
            panic!("refused evidence must not reach filesystem reader")
        }
    }

    fn measured_session(label: &str) -> AgentSession {
        let mut event = injection(label, 12);
        if let EventKind::ContextInjection { mechanism, .. } = &mut event.kind {
            *mechanism = "nested_memory".into();
        }
        event.content_measurement = Some(FixedHasher.measure(b"recorded"));
        AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::ClaudeCode,
            SessionMetadata {
                working_directory: Some(if cfg!(windows) { "C:/repo" } else { "/repo" }.into()),
                ..Default::default()
            },
            vec![event],
            vec![],
            vec![],
        )
    }

    #[test]
    fn malicious_attachment_paths_never_reach_reader() {
        for label in [
            r"\\server\share\AGENTS.md",
            "//server/share/AGENTS.md",
            r"\\?\C:\AGENTS.md",
            r"\\.\pipe\input",
            "C:AGENTS.md",
            "C:/file:stream",
            "NUL",
        ] {
            let report = compare_files(&measured_session(label), &FixedHasher, &NeverReader);
            assert_eq!(
                report.comparisons[0].status,
                InstructionFileStatus::UnsafePath,
                "{label}"
            );
            assert_eq!(report.refusal_count, 1);
            assert!(report.comparisons[0].current_digest.is_none());
        }
    }

    #[test]
    fn injected_reader_preserves_typed_refusals_and_local_comparison() {
        struct Reader(u8);
        impl InstructionFileReader for Reader {
            fn read(&self, path: &std::path::Path) -> Result<Vec<u8>, InstructionReadError> {
                assert!(ct_domain::local_paths::local_absolute(
                    &path.to_string_lossy()
                ));
                match self.0 {
                    0 => Ok(b"current".to_vec()),
                    1 => Err(InstructionReadError::TooLarge),
                    2 => Err(InstructionReadError::NotRegular),
                    3 => Err(InstructionReadError::UnsafePath),
                    _ => Err(std::io::Error::from(std::io::ErrorKind::NotFound).into()),
                }
            }
        }
        for (mode, expected) in [
            (0, InstructionFileStatus::Matching),
            (1, InstructionFileStatus::TooLarge),
            (2, InstructionFileStatus::NotRegular),
            (3, InstructionFileStatus::UnsafePath),
            (4, InstructionFileStatus::Missing),
        ] {
            let report = compare_files(&measured_session("AGENTS.md"), &FixedHasher, &Reader(mode));
            assert_eq!(report.comparisons[0].status, expected);
            assert_eq!(report.refusal_count, usize::from(mode != 0));
        }
    }
}
