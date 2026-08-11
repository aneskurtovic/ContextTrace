//! Observed instruction-artifact signatures and drift.
//!
//! The normalized event model intentionally does not retain instruction text.
//! This report therefore compares the facts the harness recorded — mechanism,
//! label and character length — rather than pretending to compare content it
//! never loaded. That makes a change useful for triage while keeping the
//! report safe to expose to an agent.

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

    let mut previous: BTreeMap<&str, &InstructionObservation> = BTreeMap::new();
    let mut changes = Vec::new();
    for observation in &observations {
        if let Some(prior) = previous.insert(&observation.mechanism, observation) {
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
        AgentKind, Event, EventId, EventKind, FileId, SessionId, SessionMetadata, SourceRef,
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
}
