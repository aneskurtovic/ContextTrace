//! Protocol activation works for running, minimized and closed installed apps.
//! The URL contains only a durable notification id; session details come from
//! the local store. A queue bridges the interval before the webview subscribes.
use super::{record_dto, NotificationRecordDto, NotificationState};
use ct_domain::ports::NotificationStore;
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::Mutex;
use tauri::{Emitter, Manager};

const EVENT: &str = "contexttrace://notification-activation";

#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Feed,
    Notification(u64),
}

fn parse(url: &str) -> Option<Target> {
    if matches!(
        url,
        "contexttrace://notifications" | "contexttrace://notifications/"
    ) {
        return Some(Target::Feed);
    }
    let id = url.strip_prefix("contexttrace://notification/")?;
    if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    id.parse()
        .ok()
        .filter(|id| *id > 0)
        .map(Target::Notification)
}

#[derive(Default)]
pub struct ActivationState(Mutex<VecDeque<Target>>);

pub fn focus(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

pub fn receive(app: &tauri::AppHandle, urls: impl IntoIterator<Item = String>) {
    let state = app.state::<ActivationState>();
    let mut queue = state
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut received = false;
    for target in urls.into_iter().filter_map(|url| parse(&url)) {
        if queue.len() == 16 {
            queue.pop_front();
        }
        queue.push_back(target);
        received = true;
    }
    drop(queue);
    if received {
        focus(app);
        let _ = app.emit(EVENT, ());
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationActivationDto {
    notification: Option<NotificationRecordDto>,
    unavailable: bool,
}

#[tauri::command]
pub fn take_notification_activations(
    activation: tauri::State<'_, ActivationState>,
    notifications: tauri::State<'_, NotificationState>,
) -> Result<Vec<NotificationActivationDto>, String> {
    // Leave requests queued if the store cannot currently be read.
    let records = notifications
        .store
        .records(None, usize::MAX)
        .map_err(|error| error.to_string())?;
    let targets: Vec<_> = activation
        .0
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .drain(..)
        .collect();
    Ok(targets
        .into_iter()
        .map(|target| match target {
            Target::Feed => NotificationActivationDto {
                notification: None,
                unavailable: false,
            },
            Target::Notification(id) => {
                let record = records.iter().find(|record| record.id == id);
                NotificationActivationDto {
                    notification: record.map(record_dto),
                    unavailable: record.is_none(),
                }
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn activation_accepts_only_our_feed_or_a_durable_numeric_id() {
        assert_eq!(
            parse("contexttrace://notification/42"),
            Some(Target::Notification(42))
        );
        assert_eq!(parse("contexttrace://notifications"), Some(Target::Feed));
        for url in [
            "https://notification/42",
            "contexttrace://notification/0",
            "contexttrace://notification/-1",
            "contexttrace://notification/42?path=C:/secret",
            "contexttrace://notification/42/extra",
            "contexttrace://notification/18446744073709551616",
            "contexttrace://notification/",
            "contexttrace://open/C:/run.exe",
        ] {
            assert_eq!(parse(url), None, "{url}");
        }
    }
}
