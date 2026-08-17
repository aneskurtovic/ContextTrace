//! Durable filesystem implementation of the notification store.
//!
//! Notification state lives below the already-disclosed ContextTrace archive
//! root. One atomically replaced JSON document keeps settings, per-session
//! cursors and feed records consistent across a crash; all access is serialized
//! through the store because the desktop poller and IPC commands run on
//! different threads.

use ct_domain::ports::{NotificationStore, PortError, PortResult};
use ct_domain::{
    AgentKind, NotificationCandidate, NotificationRecord, NotificationSettings, OsDeliveryStatus,
    SessionId, SessionNotificationCheckpoint,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

const STATE_FILE: &str = "state.json";
const TEMP_FILE: &str = "state.json.tmp";
const RETENTION_MS: u64 = 30 * 24 * 60 * 60 * 1_000;
const MAX_VISIBLE_RECORDS: usize = 2_000;

/// Presenter-only preferences that do not weaken the rule engine's typed
/// settings. They share the same durable transaction as the domain settings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationUiPreferences {
    pub onboarding_complete: bool,
    pub subagent_os_notifications: bool,
    /// Internal cursor policy: false until the first enabled discovery sweep
    /// baselines sessions that predate notifications.
    pub baseline_complete: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredNotifications {
    #[serde(default)]
    settings: NotificationSettings,
    #[serde(default)]
    ui: NotificationUiPreferences,
    #[serde(default)]
    checkpoints: Vec<SessionNotificationCheckpoint>,
    #[serde(default)]
    records: Vec<NotificationRecord>,
    #[serde(default)]
    dedupe_keys: Vec<String>,
    #[serde(default)]
    next_id: u64,
}

/// JSON-backed notification state beneath the ContextTrace-owned data root.
#[derive(Clone)]
pub struct FileNotificationStore {
    root: PathBuf,
    gate: Arc<Mutex<()>>,
}

impl FileNotificationStore {
    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            gate: Arc::new(Mutex::new(())),
        }
    }

    fn state_path(&self) -> PathBuf {
        self.root.join(STATE_FILE)
    }

    fn lock(&self) -> MutexGuard<'_, ()> {
        self.gate
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn read_unlocked(&self) -> PortResult<StoredNotifications> {
        let path = self.state_path();
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoredNotifications::default());
            }
            Err(error) => return Err(io_error(&path, error)),
        };
        let mut state: StoredNotifications =
            serde_json::from_slice(&bytes).map_err(|error| PortError::Malformed {
                path: path.display().to_string(),
                detail: error.to_string(),
            })?;
        for key in state
            .records
            .iter()
            .map(|record| &record.candidate.dedupe_key)
        {
            if !state.dedupe_keys.contains(key) {
                state.dedupe_keys.push(key.clone());
            }
        }
        state.next_id = state.next_id.max(
            state
                .records
                .iter()
                .map(|record| record.id)
                .max()
                .unwrap_or(0),
        );
        Ok(state)
    }

    fn write_unlocked(&self, state: &StoredNotifications) -> PortResult<()> {
        fs::create_dir_all(&self.root).map_err(|error| io_error(&self.root, error))?;
        let bytes = serde_json::to_vec_pretty(state)
            .map_err(|error| PortError::Io(format!("serialising notification state: {error}")))?;
        let temporary = self.root.join(TEMP_FILE);
        fs::write(&temporary, bytes).map_err(|error| io_error(&temporary, error))?;
        fs::rename(&temporary, self.state_path()).map_err(|error| io_error(&temporary, error))
    }

    fn mutate<T>(
        &self,
        operation: impl FnOnce(&mut StoredNotifications) -> PortResult<T>,
    ) -> PortResult<T> {
        let _guard = self.lock();
        let mut state = self.read_unlocked()?;
        prune_records(&mut state, now_ms());
        let result = operation(&mut state)?;
        prune_records(&mut state, now_ms());
        self.write_unlocked(&state)?;
        Ok(result)
    }

    pub fn ui_preferences(&self) -> PortResult<NotificationUiPreferences> {
        let _guard = self.lock();
        Ok(self.read_unlocked()?.ui)
    }

    pub fn save_ui_preferences(&self, preferences: NotificationUiPreferences) -> PortResult<()> {
        self.mutate(|state| {
            state.ui = preferences;
            Ok(())
        })
    }

    /// Remove feed history while retaining settings, UI preferences and
    /// checkpoints so clearing the list cannot replay old session events.
    pub fn clear_history(&self) -> PortResult<()> {
        <Self as NotificationStore>::clear(self)
    }
}

impl NotificationStore for FileNotificationStore {
    fn root(&self) -> String {
        self.root.display().to_string()
    }

    fn settings(&self) -> PortResult<NotificationSettings> {
        let _guard = self.lock();
        Ok(self.read_unlocked()?.settings)
    }

    fn save_settings(&self, settings: &NotificationSettings) -> PortResult<()> {
        self.mutate(|state| {
            state.settings = settings.clone();
            Ok(())
        })
    }

    fn checkpoint(
        &self,
        agent: AgentKind,
        session_id: &SessionId,
    ) -> PortResult<Option<SessionNotificationCheckpoint>> {
        let _guard = self.lock();
        Ok(self
            .read_unlocked()?
            .checkpoints
            .into_iter()
            .find(|checkpoint| checkpoint.agent == agent && checkpoint.session_id == *session_id))
    }

    fn save_checkpoint(&self, checkpoint: &SessionNotificationCheckpoint) -> PortResult<()> {
        self.mutate(|state| {
            if let Some(existing) = state.checkpoints.iter_mut().find(|existing| {
                existing.agent == checkpoint.agent && existing.session_id == checkpoint.session_id
            }) {
                *existing = checkpoint.clone();
            } else {
                state.checkpoints.push(checkpoint.clone());
            }
            Ok(())
        })
    }

    fn insert(
        &self,
        candidate: &NotificationCandidate,
        detected_at_ms: u64,
        catch_up: bool,
    ) -> PortResult<Option<NotificationRecord>> {
        self.mutate(|state| {
            if state.dedupe_keys.contains(&candidate.dedupe_key) {
                return Ok(None);
            }
            let id = state.next_id.saturating_add(1);
            state.next_id = id;
            let record = NotificationRecord {
                id,
                detected_at_ms,
                catch_up,
                candidate: candidate.clone(),
                read_at_ms: None,
                dismissed_at_ms: None,
                os_delivery: OsDeliveryStatus::NotRequested,
            };
            state.dedupe_keys.push(candidate.dedupe_key.clone());
            state.records.push(record.clone());
            Ok(Some(record))
        })
    }

    fn records(&self, before_id: Option<u64>, limit: usize) -> PortResult<Vec<NotificationRecord>> {
        self.mutate(|state| {
            let mut records = state.records.clone();
            records.sort_by(|left, right| right.id.cmp(&left.id));
            records.retain(|record| before_id.is_none_or(|before| record.id < before));
            records.truncate(limit);
            Ok(records)
        })
    }

    fn mark_read(&self, ids: Option<&[u64]>, read_at_ms: u64) -> PortResult<()> {
        self.mutate(|state| {
            for record in &mut state.records {
                if ids.is_none_or(|ids| ids.contains(&record.id)) {
                    record.read_at_ms.get_or_insert(read_at_ms);
                }
            }
            Ok(())
        })
    }

    fn dismiss(&self, id: u64, dismissed_at_ms: u64) -> PortResult<()> {
        self.mutate(|state| {
            let record = state
                .records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| PortError::NotFound(format!("notification {id}")))?;
            record.dismissed_at_ms.get_or_insert(dismissed_at_ms);
            Ok(())
        })
    }

    fn clear(&self) -> PortResult<()> {
        self.mutate(|state| {
            state.records.clear();
            Ok(())
        })
    }

    fn set_os_delivery(&self, id: u64, status: OsDeliveryStatus) -> PortResult<()> {
        self.mutate(|state| {
            let record = state
                .records
                .iter_mut()
                .find(|record| record.id == id)
                .ok_or_else(|| PortError::NotFound(format!("notification {id}")))?;
            record.os_delivery = status;
            Ok(())
        })
    }
}

fn io_error(path: &Path, error: std::io::Error) -> PortError {
    PortError::Io(format!("{}: {error}", path.display()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn prune_records(state: &mut StoredNotifications, now_ms: u64) {
    let cutoff = now_ms.saturating_sub(RETENTION_MS);
    state
        .records
        .retain(|record| record.detected_at_ms >= cutoff);
    if state.records.len() > MAX_VISIBLE_RECORDS {
        state.records.sort_by_key(|record| record.id);
        let remove = state.records.len() - MAX_VISIBLE_RECORDS;
        state.records.drain(..remove);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::{
        NotificationDelivery, NotificationEvidence, NotificationLocation, NotificationRuleId,
        NotificationSeverity,
    };
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let nonce = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos();
            Self(std::env::temp_dir().join(format!("ct-notifications-{nonce}")))
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn candidate(key: &str) -> NotificationCandidate {
        NotificationCandidate {
            dedupe_key: key.into(),
            rule: NotificationRuleId::Compaction,
            severity: NotificationSeverity::Info,
            delivery: NotificationDelivery::FeedOnly,
            title: "Context compacted".into(),
            body: "Compaction reclaimed prompt space.".into(),
            occurred_at_ms: Some(40),
            location: NotificationLocation {
                agent: AgentKind::Codex,
                session_id: SessionId::new("session").expect("valid id"),
                project: Some("C:\\work".into()),
                turn: None,
                line: Some(8),
            },
            evidence: NotificationEvidence::Compaction {
                trigger: Some("auto".into()),
                tokens_before: Some(80_000),
                tokens_after: Some(20_000),
                reclaimed: Some(60_000),
            },
        }
    }

    #[test]
    fn state_survives_a_new_store_instance_and_insert_is_idempotent() {
        let scratch = Scratch::new();
        let store = FileNotificationStore::at(&scratch.0);
        let detected = now_ms();
        let first = store
            .insert(&candidate("same-event"), detected, false)
            .unwrap()
            .unwrap();
        assert_eq!(first.id, 1);
        assert!(store
            .insert(&candidate("same-event"), detected.saturating_add(1), false)
            .unwrap()
            .is_none());

        let reopened = FileNotificationStore::at(&scratch.0);
        assert_eq!(reopened.records(None, 10).unwrap(), vec![first]);
    }

    #[test]
    fn clearing_history_preserves_preferences_and_checkpoints() {
        let scratch = Scratch::new();
        let store = FileNotificationStore::at(&scratch.0);
        let preferences = NotificationUiPreferences {
            onboarding_complete: true,
            subagent_os_notifications: true,
            baseline_complete: true,
        };
        store.save_ui_preferences(preferences).unwrap();
        let detected = now_ms();
        let checkpoint = SessionNotificationCheckpoint::baseline(
            AgentKind::Codex,
            SessionId::new("session").unwrap(),
        );
        store.save_checkpoint(&checkpoint).unwrap();
        store.insert(&candidate("event"), detected, false).unwrap();

        store.clear_history().unwrap();

        assert!(store.records(None, 10).unwrap().is_empty());
        assert_eq!(store.ui_preferences().unwrap(), preferences);
        assert_eq!(
            store
                .checkpoint(AgentKind::Codex, &checkpoint.session_id)
                .unwrap(),
            Some(checkpoint)
        );
        assert!(store
            .insert(&candidate("event"), detected.saturating_add(1), true)
            .unwrap()
            .is_none());
        let next = store
            .insert(&candidate("new-event"), detected.saturating_add(2), false)
            .unwrap()
            .unwrap();
        assert_eq!(next.id, 2, "clearing history does not reuse durable ids");
    }

    #[test]
    fn pruning_keeps_only_thirty_days_and_the_latest_two_thousand_records() {
        let mut state = StoredNotifications {
            records: (1..=2_010)
                .map(|id| NotificationRecord {
                    id,
                    detected_at_ms: if id == 1 { 1 } else { RETENTION_MS + id },
                    catch_up: false,
                    candidate: candidate(&format!("event-{id}")),
                    read_at_ms: None,
                    dismissed_at_ms: None,
                    os_delivery: OsDeliveryStatus::NotRequested,
                })
                .collect(),
            ..StoredNotifications::default()
        };

        prune_records(&mut state, RETENTION_MS * 2);

        assert_eq!(state.records.len(), MAX_VISIBLE_RECORDS);
        assert_eq!(state.records.first().map(|record| record.id), Some(11));
        assert_eq!(state.records.last().map(|record| record.id), Some(2_010));
    }
}
