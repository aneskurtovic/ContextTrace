//! Session-family grouping over the parent identities adapters already expose.

use ct_domain::{SessionDescriptor, ThreadRole};
use serde::Serialize;
use std::collections::BTreeMap;

/// One root and every discovered branch that names it.
///
/// `root` is optional because callers may filter the corpus or because an
/// agent can retain a child after its parent log has been deleted. Such a
/// branch is still useful and must remain separately inspectable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionFamily {
    pub root_id: String,
    pub root: Option<SessionDescriptor>,
    pub branches: Vec<SessionDescriptor>,
}

/// Group descriptors by their recorded thread parent.
pub fn families(sessions: &[SessionDescriptor]) -> Vec<SessionFamily> {
    let mut roots: BTreeMap<String, SessionDescriptor> = BTreeMap::new();
    let mut branches: BTreeMap<String, Vec<SessionDescriptor>> = BTreeMap::new();

    for session in sessions {
        match &session.thread_role {
            ThreadRole::Root => {
                roots.insert(session.id.to_string(), session.clone());
            }
            ThreadRole::Subagent { parent } => {
                branches
                    .entry(parent.to_string())
                    .or_default()
                    .push(session.clone());
            }
        }
    }

    let mut ids: Vec<String> = roots.keys().chain(branches.keys()).cloned().collect();
    ids.sort();
    ids.dedup();

    ids.into_iter()
        .map(|root_id| {
            let mut branches = branches.remove(&root_id).unwrap_or_default();
            branches.sort_by(|a, b| {
                a.started_at
                    .cmp(&b.started_at)
                    .then_with(|| a.id.as_str().cmp(b.id.as_str()))
            });
            SessionFamily {
                root: roots.get(&root_id).cloned(),
                root_id,
                branches,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::{AgentKind, SessionId, ThreadRole};

    fn descriptor(id: &str, role: ThreadRole) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new(id).unwrap(),
            agent: AgentKind::Codex,
            path: format!("{id}.jsonl"),
            size_bytes: 1,
            project: None,
            started_at: None,
            last_activity: None,
            thread_role: role,
        }
    }

    #[test]
    fn groups_branches_under_their_parent_and_keeps_orphans() {
        let sessions = vec![
            descriptor(
                "child-b",
                ThreadRole::Subagent {
                    parent: SessionId::new("root").unwrap(),
                },
            ),
            descriptor(
                "orphan",
                ThreadRole::Subagent {
                    parent: SessionId::new("missing").unwrap(),
                },
            ),
            descriptor("root", ThreadRole::Root),
            descriptor(
                "child-a",
                ThreadRole::Subagent {
                    parent: SessionId::new("root").unwrap(),
                },
            ),
        ];
        let trees = families(&sessions);
        assert_eq!(trees.len(), 2);
        assert_eq!(trees[0].root_id, "missing");
        assert!(trees[0].root.is_none());
        assert_eq!(trees[1].root_id, "root");
        assert_eq!(trees[1].branches.len(), 2);
    }

    #[test]
    fn a_root_without_branches_is_still_a_family() {
        let trees = families(&[descriptor("root", ThreadRole::Root)]);
        assert_eq!(trees[0].root_id, "root");
        assert!(trees[0].branches.is_empty());
        assert!(trees[0].root.is_some());
    }
}
