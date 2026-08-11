//! Per-turn parse-fidelity trends.
//!
//! The session-wide fidelity score answers whether anything was missed. This
//! module adds the time dimension: when did the parser start seeing shapes it
//! did not understand? It deliberately counts only events assigned to a turn;
//! lifecycle records without a turn remain in `unassigned_events` rather than
//! being silently attached to an arbitrary request.

use ct_domain::{AgentSession, EventKind};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FidelityPoint {
    pub turn: u32,
    pub events: u32,
    pub unrecognised_events: u32,
    pub fidelity: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FidelityTrend {
    pub session_id: String,
    pub points: Vec<FidelityPoint>,
    pub unassigned_events: u32,
    pub unassigned_unrecognised: u32,
}

impl FidelityTrend {
    pub fn is_clean(&self) -> bool {
        self.points
            .iter()
            .all(|point| point.unrecognised_events == 0)
            && self.unassigned_unrecognised == 0
    }
}

pub fn trend(session: &AgentSession) -> FidelityTrend {
    let mut by_turn: BTreeMap<u32, (u32, u32)> = BTreeMap::new();
    let mut unassigned_events = 0;
    let mut unassigned_unrecognised = 0;

    for event in session.events() {
        let unrecognised = u32::from(matches!(event.kind, EventKind::Unrecognised));
        let Some(turn) = event.turn.map(|turn| turn.get()) else {
            unassigned_events += 1;
            unassigned_unrecognised += unrecognised;
            continue;
        };
        let entry = by_turn.entry(turn).or_default();
        entry.0 += 1;
        entry.1 += unrecognised;
    }

    let points = by_turn
        .into_iter()
        .map(|(turn, (events, unrecognised_events))| FidelityPoint {
            turn,
            events,
            unrecognised_events,
            fidelity: fidelity(events, unrecognised_events),
        })
        .collect();

    FidelityTrend {
        session_id: session.id().to_string(),
        points,
        unassigned_events,
        unassigned_unrecognised,
    }
}

fn fidelity(events: u32, unrecognised: u32) -> f32 {
    if events == 0 {
        1.0
    } else {
        1.0 - (unrecognised as f32 / events as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::model::event::EventLinks;
    use ct_domain::{
        AgentKind, Event, EventId, EventKind, FileId, SessionId, SessionMetadata, SourceRef,
        TurnNumber,
    };

    fn event(turn: Option<u32>, kind: EventKind) -> Event {
        Event {
            id: EventId::Ordinal(0),
            sequence: 0,
            timestamp: None,
            kind,
            source: SourceRef::new(FileId(0), 0, 0, 1),
            raw_type: "test".into(),
            turn: turn.and_then(|turn| TurnNumber::new(turn).ok()),
            links: EventLinks::default(),
            content_measurement: None,
        }
    }

    #[test]
    fn points_show_where_unrecognised_events_appear() {
        let session = AgentSession::new(
            SessionId::new("s").unwrap(),
            AgentKind::Codex,
            SessionMetadata::default(),
            vec![
                event(Some(1), EventKind::SessionStarted),
                event(Some(1), EventKind::Unrecognised),
                event(Some(2), EventKind::SessionStarted),
                event(None, EventKind::Unrecognised),
            ],
            vec![],
            vec![],
        );
        let trend = trend(&session);
        assert_eq!(trend.points[0].turn, 1);
        assert_eq!(trend.points[0].unrecognised_events, 1);
        assert!((trend.points[0].fidelity - 0.5).abs() < f32::EPSILON);
        assert_eq!(trend.unassigned_unrecognised, 1);
    }
}
