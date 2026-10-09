//! Lightweight live-file observation independent of notification configuration.
//! Descriptor fingerprints do not require parsing/calibrating session bodies.
use ct_domain::{AgentKind, SessionDescriptor, SessionFingerprint};
use std::collections::{BTreeMap, BTreeSet};

type SessionKey = (AgentKind, String);

#[derive(Default)]
pub(super) struct SessionObserver {
    observed: BTreeMap<SessionKey, SessionFingerprint>,
    pending: BTreeSet<SessionKey>,
}

impl SessionObserver {
    /// Only call for a successfully discovered agent inventory. An unavailable
    /// source is not an empty inventory and must not manufacture deletions.
    pub(super) fn refresh_agent(&mut self, agent: AgentKind, descriptors: &[SessionDescriptor]) {
        let mut current = BTreeMap::new();
        for descriptor in descriptors
            .iter()
            .filter(|descriptor| descriptor.agent == agent)
        {
            let key = (agent, descriptor.id.to_string());
            let fingerprint = SessionFingerprint {
                path: descriptor.path.clone(),
                size_bytes: descriptor.size_bytes,
                last_activity: descriptor.last_activity,
            };
            current.insert(key, fingerprint);
        }
        for (key, fingerprint) in &current {
            if self.observed.get(key) != Some(fingerprint) {
                self.pending.insert(key.clone());
            }
        }
        for key in self.observed.keys().filter(|key| key.0 == agent) {
            if !current.contains_key(key) {
                self.pending.insert(key.clone());
            }
        }
        self.observed.retain(|key, _| key.0 != agent);
        self.observed.extend(current);
    }

    /// A failed UI delivery remains pending for retry on the next poll.
    pub(super) fn deliver_pending(
        &mut self,
        mut deliver: impl FnMut(AgentKind, &str) -> Result<(), String>,
    ) -> Result<(), String> {
        for key in self.pending.clone() {
            deliver(key.0, &key.1)?;
            self.pending.remove(&key);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::{SessionId, ThreadRole};

    fn descriptor(agent: AgentKind, path: &str, size_bytes: u64) -> SessionDescriptor {
        SessionDescriptor {
            id: SessionId::new("shared").unwrap(),
            agent,
            path: path.into(),
            size_bytes,
            project: None,
            title: None,
            git_branch: None,
            started_at: None,
            last_activity: None,
            thread_role: ThreadRole::Root,
        }
    }
    fn take(observer: &mut SessionObserver) -> Vec<SessionKey> {
        let mut delivered = Vec::new();
        observer
            .deliver_pending(|agent, id| {
                delivered.push((agent, id.into()));
                Ok(())
            })
            .unwrap();
        delivered
    }

    #[test]
    fn initial_append_rotation_and_deletion_are_coalesced_without_alert_settings() {
        let agent = AgentKind::Codex;
        let mut observer = SessionObserver::default();
        let mut file = descriptor(agent, "/log/one", 10);
        observer.refresh_agent(agent, &[file.clone()]);
        assert_eq!(take(&mut observer), vec![(agent, "shared".into())]);
        observer.refresh_agent(agent, &[file.clone()]);
        assert!(take(&mut observer).is_empty());
        file.size_bytes = 20;
        observer.refresh_agent(agent, &[file.clone()]);
        file.size_bytes = 30;
        observer.refresh_agent(agent, &[file.clone()]);
        assert_eq!(take(&mut observer).len(), 1, "coalesce before delivery");
        file.path = "/log/two".into();
        file.size_bytes = 5;
        observer.refresh_agent(agent, &[file.clone()]);
        assert_eq!(take(&mut observer).len(), 1, "rotation");
        observer.refresh_agent(agent, &[]);
        assert_eq!(take(&mut observer).len(), 1, "deletion");
        observer.refresh_agent(agent, &[]);
        assert!(take(&mut observer).is_empty());
    }

    #[test]
    fn same_id_in_two_agents_has_independent_observation_and_invalidation() {
        let mut observer = SessionObserver::default();
        let codex = descriptor(AgentKind::Codex, "/codex", 10);
        let claude = descriptor(AgentKind::ClaudeCode, "/claude", 10);
        observer.refresh_agent(codex.agent, std::slice::from_ref(&codex));
        observer.refresh_agent(claude.agent, std::slice::from_ref(&claude));
        let mut cache = BTreeSet::from([
            (codex.agent, "shared".to_string()),
            (claude.agent, "shared".to_string()),
        ]);
        take(&mut observer);
        observer.refresh_agent(codex.agent, &[descriptor(codex.agent, "/codex", 20)]);
        observer
            .deliver_pending(|agent, id| {
                cache.remove(&(agent, id.into()));
                Ok(())
            })
            .unwrap();
        assert!(cache.contains(&(claude.agent, "shared".into())));
        assert!(!cache.contains(&(codex.agent, "shared".into())));
        // The failed Claude discovery is deliberately not supplied as an empty
        // successful refresh; its inventory survives another agent's poll.
        observer.refresh_agent(codex.agent, &[]);
        assert_eq!(take(&mut observer), vec![(codex.agent, "shared".into())]);
        observer.refresh_agent(claude.agent, &[claude]);
        assert!(take(&mut observer).is_empty());
    }

    #[test]
    fn changed_timestamp_with_same_size_refreshes_and_failed_emission_retries() {
        let agent = AgentKind::Codex;
        let mut observer = SessionObserver::default();
        let mut file = descriptor(agent, "/log", 10);
        observer.refresh_agent(agent, &[file.clone()]);
        take(&mut observer);
        file.last_activity = Some(chrono::DateTime::from_timestamp(123, 0).unwrap());
        observer.refresh_agent(agent, &[file.clone()]);
        assert!(observer
            .deliver_pending(|_, _| Err("temporary emit failure".into()))
            .is_err());
        observer.refresh_agent(agent, &[file]);
        assert_eq!(take(&mut observer).len(), 1);
        assert!(take(&mut observer).is_empty());
    }
}
