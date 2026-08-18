//! Notification persistence, IPC presentation and live-session monitoring.

mod delivery;

use super::AppState;
use chrono::{DateTime, Utc};
use ct_domain::ports::NotificationStore;
use ct_domain::{
    AgentKind, NotificationDelivery, NotificationRecord, NotificationRuleId, NotificationSettings,
    OsDeliveryStatus, ThreadRole,
};
use delivery::Deliverability;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::plugin::PermissionState;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

const POLL_INTERVAL: Duration = Duration::from_millis(2_500);
const DEFAULT_PAGE_SIZE: usize = 30;
const MAX_PAGE_SIZE: usize = 200;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuleSettingDto {
    delivery: String,
    threshold: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettingsDto {
    enabled: bool,
    onboarding_complete: bool,
    subagent_os_notifications: bool,
    rules: BTreeMap<String, RuleSettingDto>,
    cost_budget_usd: Option<f64>,
}

/// What the desktop can say about OS delivery *before* anything is sent.
///
/// `os_permission` alone was misleading on Windows: the plugin answers
/// `Granted` unconditionally there, so a settings panel reporting it was
/// telling every user that toasts would work regardless of whether this build
/// could produce one. `deliverability` is the observation that question
/// actually needs, and `obstacle` is the sentence to show when it is negative.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationStatusDto {
    monitoring: bool,
    os_permission: String,
    last_successful_poll: Option<String>,
    error: Option<String>,
    deliverability: Deliverability,
    obstacle: Option<String>,
}

/// The outcome of one deliberately triggered toast.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TestNotificationDto {
    delivered: bool,
    /// Windows' own words when it refused, or this build's reason for not
    /// asking it. `None` only when the toast was accepted.
    reason: Option<String>,
    deliverability: Deliverability,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationLocationDto {
    agent: String,
    session_id: String,
    project: Option<String>,
    turn: Option<u32>,
    source_line: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecordDto {
    id: String,
    rule_id: String,
    severity: String,
    delivery: String,
    confidence: String,
    title: String,
    description: String,
    occurred_at: String,
    detected_at: String,
    read_at: Option<String>,
    dismissed_at: Option<String>,
    catch_up: bool,
    location: NotificationLocationDto,
    /// What became of the OS toast for this record: `notRequested` when the
    /// rule is feed-only or the record was caught up after the fact,
    /// `delivered` when Windows accepted it, `failed` with the reason
    /// otherwise. Presented rather than kept internal because a feed row
    /// claiming an OS notification the user never saw is precisely the
    /// confusion this pass exists to end.
    os_delivery: OsDeliveryDto,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OsDeliveryDto {
    NotRequested,
    Delivered,
    Failed { reason: String },
}

impl From<&OsDeliveryStatus> for OsDeliveryDto {
    fn from(value: &OsDeliveryStatus) -> Self {
        match value {
            OsDeliveryStatus::NotRequested => OsDeliveryDto::NotRequested,
            OsDeliveryStatus::Delivered => OsDeliveryDto::Delivered,
            OsDeliveryStatus::Failed { reason } => OsDeliveryDto::Failed {
                reason: reason.clone(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationPageDto {
    notifications: Vec<NotificationRecordDto>,
    next_cursor: Option<String>,
    unread_count: usize,
}

#[derive(Default)]
struct MonitorStatus {
    running: bool,
    last_successful_poll: Option<DateTime<Utc>>,
    error: Option<String>,
}

pub struct NotificationState {
    store: ct_runtime::FileNotificationStore,
    status: Mutex<MonitorStatus>,
    seen_this_run: Mutex<HashSet<(AgentKind, String)>>,
    /// Resolved once per process. Working it out reads the Start Menu, and the
    /// answer only changes when the app is installed or moved -- neither of
    /// which happens to a running process without restarting it.
    deliverability: OnceLock<Deliverability>,
}

impl NotificationState {
    pub fn new() -> Self {
        Self {
            store: ct_runtime::notification_store(),
            status: Mutex::new(MonitorStatus::default()),
            seen_this_run: Mutex::new(HashSet::new()),
            deliverability: OnceLock::new(),
        }
    }

    fn status(&self) -> MutexGuard<'_, MonitorStatus> {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn seen_this_run(&self) -> MutexGuard<'_, HashSet<(AgentKind, String)>> {
        self.seen_this_run
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn deliverability(&self, app: &AppHandle) -> &Deliverability {
        self.deliverability.get_or_init(|| {
            let config = app.config();
            delivery::deliverability(
                &config.identifier,
                config.product_name.as_deref().unwrap_or("ContextTrace"),
            )
        })
    }
}

pub fn start_monitor(app: AppHandle) {
    std::thread::spawn(move || loop {
        let result = poll(&app);
        let notifications = app.state::<NotificationState>();
        let enabled = notifications
            .store
            .settings()
            .map(|settings| settings.enabled)
            .unwrap_or(false);
        let mut status = notifications.status();
        status.running = enabled;
        match result {
            Ok(()) => {
                status.last_successful_poll = Some(DateTime::<Utc>::from(SystemTime::now()));
                status.error = None;
            }
            Err(error) => status.error = Some(error),
        }
        drop(status);
        std::thread::sleep(POLL_INTERVAL);
    });
}

fn poll(app: &AppHandle) -> Result<(), String> {
    let notifications = app.state::<NotificationState>();
    let settings = notifications
        .store
        .settings()
        .map_err(|error| error.to_string())?;
    if !settings.enabled {
        notifications.seen_this_run().clear();
        return Ok(());
    }
    let mut preferences = notifications
        .store
        .ui_preferences()
        .map_err(|error| error.to_string())?;
    let baseline_mode = !preferences.baseline_complete;

    let state = app.state::<AppState>();
    let descriptors = state.app.list_sessions(&ct_application::SessionFilter {
        agent: None,
        project: None,
        since: None,
        limit: None,
    });
    for descriptor in descriptors {
        poll_session(
            app,
            &state,
            &notifications,
            &settings,
            descriptor,
            baseline_mode,
        )?;
    }
    if baseline_mode {
        preferences.baseline_complete = true;
        notifications
            .store
            .save_ui_preferences(preferences)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn poll_session(
    app: &AppHandle,
    state: &AppState,
    notifications: &NotificationState,
    settings: &NotificationSettings,
    descriptor: ct_domain::SessionDescriptor,
    baseline_mode: bool,
) -> Result<(), String> {
    let fingerprint = ct_domain::SessionFingerprint {
        path: descriptor.path.clone(),
        size_bytes: descriptor.size_bytes,
        last_activity: descriptor.last_activity,
    };
    let prior = notifications
        .store
        .checkpoint(descriptor.agent, &descriptor.id)
        .map_err(|error| error.to_string())?;
    let key = (descriptor.agent, descriptor.id.to_string());
    let first_seen_this_run = notifications.seen_this_run().insert(key);
    if prior
        .as_ref()
        .and_then(|checkpoint| checkpoint.fingerprint.as_ref())
        == Some(&fingerprint)
    {
        return Ok(());
    }
    let rotated = prior
        .as_ref()
        .and_then(|checkpoint| checkpoint.fingerprint.as_ref())
        .is_some_and(|previous| {
            previous.path != fingerprint.path || fingerprint.size_bytes < previous.size_bytes
        });

    state.invalidate_session(descriptor.agent, descriptor.id.as_str());
    let cached = state.cached_session(
        descriptor.agent,
        descriptor.id.as_str(),
        prior.is_some() || !baseline_mode,
    )?;
    let sequence_rewound = prior
        .as_ref()
        .and_then(|checkpoint| checkpoint.last_sequence)
        .zip(cached.session.events().last().map(|event| event.sequence))
        .is_some_and(|(previous, current)| current < previous);
    if baseline_mode || rotated || sequence_rewound {
        let mut baseline =
            ct_application::NotificationEngine::baseline_with_settings(&cached.session, settings);
        baseline.fingerprint = Some(fingerprint);
        notifications
            .store
            .save_checkpoint(&baseline)
            .map_err(|error| error.to_string())?;
        if !baseline_mode {
            app.emit(
                "contexttrace://session-updated",
                SessionUpdatedEvent {
                    agent: descriptor.agent.to_string(),
                    session_id: descriptor.id.to_string(),
                },
            )
            .map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    let prior = prior.unwrap_or_else(|| {
        ct_domain::SessionNotificationCheckpoint::baseline(descriptor.agent, descriptor.id.clone())
    });

    let fitted = cached
        .chars_per_token()
        .map(ct_runtime::heuristic_estimator);
    let estimator: &dyn ct_domain::ports::TokenEstimator = match fitted.as_ref() {
        Some(estimator) => estimator,
        None => state.app.binding_estimator(cached.binding),
    };
    let wants_snapshots = settings.large_contributor.delivery.enabled()
        || settings.duplicate_context.delivery.enabled()
        || settings.low_entropy_content.delivery.enabled();
    let snapshots = if wants_snapshots {
        cached
            .session
            .turns()
            .iter()
            .filter(|turn| prior.last_turn.is_none_or(|last| turn.number > last))
            .filter_map(|turn| {
                state
                    .app
                    .snapshot_with(&cached.session, cached.binding, turn.number, estimator)
                    .ok()
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let secret_scan = if settings.secret_exposure.delivery.enabled() {
        let raw = ct_runtime::raw_event_source(&cached.descriptor.path);
        Some(state.app.scan_secrets(&cached.session, &raw))
    } else {
        None
    };
    let residual_steps = if settings.residual_step.delivery.enabled() {
        cached
            .ratio
            .map(|ratio| {
                let points = state
                    .app
                    .residual_series(&cached.session, cached.binding, ratio);
                ct_application::residual_steps(&points)
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let instruction_drift = settings
        .instruction_drift
        .delivery
        .enabled()
        .then(|| ct_application::instruction_drift(&cached.session));
    let cost = if settings.cost_budget.delivery.enabled() && settings.cost_budget_micros.is_some() {
        let report = ct_application::project_cost_scenario(
            &cached.session,
            &ct_application::CostScenario {
                forecast_turns: Some(10),
                ..ct_application::CostScenario::default()
            },
        );
        Some(ct_application::CostBudgetObservation {
            observed_micros: report.total.0,
            projected_micros: report.forecast.map(|forecast| forecast.projected_total.0),
        })
    } else {
        None
    };
    let mut evaluation = ct_application::NotificationEngine::evaluate(
        &cached.session,
        &prior,
        settings,
        ct_application::NotificationInputs {
            snapshots: &snapshots,
            secret_findings: secret_scan
                .as_ref()
                .map(|scan| scan.findings.as_slice())
                .unwrap_or_default(),
            residual_steps: &residual_steps,
            instruction_drift: instruction_drift.as_ref(),
            cost,
        },
    );
    let catch_up = first_seen_this_run && prior.fingerprint.is_some();
    let preferences = notifications
        .store
        .ui_preferences()
        .map_err(|error| error.to_string())?;
    for candidate in evaluation.candidates {
        let Some(mut record) = notifications
            .store
            .insert(&candidate, now_ms(), catch_up)
            .map_err(|error| error.to_string())?
        else {
            continue;
        };
        app.emit("contexttrace://notification-created", record_dto(&record))
            .map_err(|error| error.to_string())?;
        if !catch_up
            && candidate.delivery.includes_os()
            && subagent_os_allowed(&descriptor.thread_role, preferences)
        {
            let os_status = deliver_os(app, notifications, &record);
            notifications
                .store
                .set_os_delivery(record.id, os_status.clone())
                .map_err(|error| error.to_string())?;
            record.os_delivery = os_status;
            app.emit("contexttrace://notification-updated", record_dto(&record))
                .map_err(|error| error.to_string())?;
        }
    }
    evaluation.checkpoint.fingerprint = Some(fingerprint);
    notifications
        .store
        .save_checkpoint(&evaluation.checkpoint)
        .map_err(|error| error.to_string())?;
    app.emit(
        "contexttrace://session-updated",
        SessionUpdatedEvent {
            agent: descriptor.agent.to_string(),
            session_id: descriptor.id.to_string(),
        },
    )
    .map_err(|error| error.to_string())?;
    Ok(())
}

#[tauri::command]
pub fn get_notification_settings(
    state: tauri::State<'_, NotificationState>,
) -> Result<NotificationSettingsDto, String> {
    settings_dto(&state)
}

#[tauri::command]
pub fn update_notification_settings(
    settings: NotificationSettingsDto,
    app: AppHandle,
    state: tauri::State<'_, NotificationState>,
) -> Result<NotificationSettingsDto, String> {
    let current = state.store.settings().map_err(|error| error.to_string())?;
    let was_enabled = current.enabled;
    let domain = settings_to_domain(&settings, current)?;
    state
        .store
        .save_settings(&domain)
        .map_err(|error| error.to_string())?;
    let existing_ui = state
        .store
        .ui_preferences()
        .map_err(|error| error.to_string())?;
    state
        .store
        .save_ui_preferences(ct_runtime::NotificationUiPreferences {
            onboarding_complete: settings.onboarding_complete,
            subagent_os_notifications: settings.subagent_os_notifications,
            baseline_complete: if was_enabled && !domain.enabled {
                false
            } else {
                existing_ui.baseline_complete
            },
        })
        .map_err(|error| error.to_string())?;

    if domain.enabled
        && domain_has_os_delivery(&domain)
        && matches!(
            app.notification().permission_state(),
            Ok(PermissionState::Prompt)
        )
    {
        let _ = app.notification().request_permission();
    }
    state.status().running = domain.enabled;
    settings_dto(&state)
}

#[tauri::command]
pub fn get_notification_status(
    app: AppHandle,
    state: tauri::State<'_, NotificationState>,
) -> NotificationStatusDto {
    let deliverability = state.deliverability(&app).clone();
    let permission = permission_label(app.notification().permission_state());
    let status = state.status();
    NotificationStatusDto {
        monitoring: status.running,
        os_permission: permission.into(),
        last_successful_poll: status.last_successful_poll.map(|value| value.to_rfc3339()),
        error: status.error.clone(),
        obstacle: deliverability.obstacle(),
        deliverability,
    }
}

/// Send a toast on demand, so "do OS notifications work here" stops being a
/// question answered by waiting for one of eleven rules to fire.
///
/// It reports the true outcome, including the case where nothing was sent
/// because this build cannot deliver. Nothing is written to the feed: a test
/// the user asked for is not an observation about their sessions.
#[tauri::command]
pub fn send_test_notification(
    app: AppHandle,
    state: tauri::State<'_, NotificationState>,
) -> TestNotificationDto {
    let deliverability = state.deliverability(&app).clone();
    let outcome = delivery::deliver(
        &deliverability,
        "ContextTrace test notification",
        "If you can see this, OS notifications are reaching you.",
    );
    TestNotificationDto {
        delivered: matches!(outcome, OsDeliveryStatus::Delivered),
        reason: match outcome {
            OsDeliveryStatus::Failed { reason } => Some(reason),
            OsDeliveryStatus::Delivered | OsDeliveryStatus::NotRequested => None,
        },
        deliverability,
    }
}

#[tauri::command]
pub fn list_notifications(
    before_id: Option<String>,
    limit: Option<usize>,
    unread_only: Option<bool>,
    state: tauri::State<'_, NotificationState>,
) -> Result<NotificationPageDto, String> {
    let before = before_id
        .map(|value| {
            value
                .parse::<u64>()
                .map_err(|_| "invalid notification cursor".to_string())
        })
        .transpose()?;
    let limit = limit.unwrap_or(DEFAULT_PAGE_SIZE).clamp(1, MAX_PAGE_SIZE);
    let all = state
        .store
        .records(None, usize::MAX)
        .map_err(|error| error.to_string())?;
    let unread_count = all
        .iter()
        .filter(|record| !record.is_read() && !record.is_dismissed())
        .count();
    let mut visible: Vec<_> = all
        .into_iter()
        .filter(|record| !record.is_dismissed())
        .filter(|record| before.is_none_or(|cursor| record.id < cursor))
        .filter(|record| !unread_only.unwrap_or(false) || !record.is_read())
        .collect();
    let has_more = visible.len() > limit;
    visible.truncate(limit);
    let next_cursor = has_more
        .then(|| visible.last().map(|record| record.id.to_string()))
        .flatten();
    Ok(NotificationPageDto {
        notifications: visible.iter().map(record_dto).collect(),
        next_cursor,
        unread_count,
    })
}

#[tauri::command]
pub fn mark_notifications_read(
    ids: Option<Vec<String>>,
    app: AppHandle,
    state: tauri::State<'_, NotificationState>,
) -> Result<(), String> {
    let ids = parse_ids(ids)?;
    state
        .store
        .mark_read(ids.as_deref(), now_ms())
        .map_err(|error| error.to_string())?;
    emit_updated_records(&app, &state, ids.as_deref())
}

#[tauri::command]
pub fn dismiss_notification(
    id: String,
    app: AppHandle,
    state: tauri::State<'_, NotificationState>,
) -> Result<(), String> {
    let id = id
        .parse::<u64>()
        .map_err(|_| "invalid notification id".to_string())?;
    state
        .store
        .dismiss(id, now_ms())
        .map_err(|error| error.to_string())?;
    emit_updated_records(&app, &state, Some(&[id]))
}

#[tauri::command]
pub fn clear_notification_history(
    state: tauri::State<'_, NotificationState>,
) -> Result<(), String> {
    state.store.clear().map_err(|error| error.to_string())
}

fn settings_dto(state: &NotificationState) -> Result<NotificationSettingsDto, String> {
    let settings = state.store.settings().map_err(|error| error.to_string())?;
    let ui = state
        .store
        .ui_preferences()
        .map_err(|error| error.to_string())?;
    Ok(domain_to_settings(&settings, ui))
}

fn domain_to_settings(
    settings: &NotificationSettings,
    ui: ct_runtime::NotificationUiPreferences,
) -> NotificationSettingsDto {
    let mut rules = BTreeMap::new();
    let mut add = |id: &str, delivery, threshold| {
        rules.insert(
            id.into(),
            RuleSettingDto {
                delivery: delivery_label(delivery).into(),
                threshold,
            },
        );
    };
    add(
        "contextPressure",
        settings.context_pressure.delivery,
        Some(f64::from(settings.context_pressure.warning) * 100.0),
    );
    add(
        "promptSpike",
        settings.prompt_spike.delivery,
        Some(f64::from(settings.prompt_spike.tokens)),
    );
    add(
        "toolErrorStreak",
        settings.tool_error_streak.delivery,
        Some(f64::from(settings.tool_error_streak.count)),
    );
    add("secretExposure", settings.secret_exposure.delivery, None);
    add("formatDrift", settings.format_drift.delivery, None);
    add("compaction", settings.compaction.delivery, None);
    add(
        "contextDominance",
        settings.large_contributor.delivery,
        Some(f64::from(settings.large_contributor.share) * 100.0),
    );
    add(
        "residualStep",
        settings.residual_step.delivery,
        Some(f64::from(settings.residual_step.tokens)),
    );
    add(
        "instructionDrift",
        settings.instruction_drift.delivery,
        None,
    );
    add(
        "contextWaste",
        settings.duplicate_context.delivery,
        Some(f64::from(
            settings
                .duplicate_context
                .tokens
                .max(settings.low_entropy_content.tokens),
        )),
    );
    add(
        "costBudget",
        if settings.cost_budget_micros.is_some() {
            settings.cost_budget.delivery
        } else {
            NotificationDelivery::Off
        },
        None,
    );
    NotificationSettingsDto {
        enabled: settings.enabled,
        onboarding_complete: ui.onboarding_complete,
        subagent_os_notifications: ui.subagent_os_notifications,
        rules,
        cost_budget_usd: settings
            .cost_budget_micros
            .map(|value| value as f64 / 1_000_000.0),
    }
}

fn settings_to_domain(
    dto: &NotificationSettingsDto,
    mut settings: NotificationSettings,
) -> Result<NotificationSettings, String> {
    settings.enabled = dto.enabled;
    settings.context_pressure.delivery = rule(dto, "contextPressure")?.delivery()?;
    let pressure = rule(dto, "contextPressure")?.threshold(75.0)? / 100.0;
    if pressure >= f64::from(settings.context_pressure.critical) {
        return Err(format!(
            "context pressure warning must stay below the {:.0}% critical threshold",
            settings.context_pressure.critical * 100.0
        ));
    }
    settings.context_pressure.warning = pressure as f32;
    settings.context_pressure.reset_below_warning = (pressure - 0.05).max(0.0) as f32;
    settings.prompt_spike.delivery = rule(dto, "promptSpike")?.delivery()?;
    settings.prompt_spike.tokens = whole_threshold(rule(dto, "promptSpike")?, 20_000)?;
    settings.tool_error_streak.delivery = rule(dto, "toolErrorStreak")?.delivery()?;
    settings.tool_error_streak.count = whole_threshold(rule(dto, "toolErrorStreak")?, 3)?;
    settings.secret_exposure.delivery = rule(dto, "secretExposure")?.delivery()?;
    settings.format_drift.delivery = rule(dto, "formatDrift")?.delivery()?;
    settings.compaction.delivery = rule(dto, "compaction")?.delivery()?;
    settings.large_contributor.delivery = rule(dto, "contextDominance")?.delivery()?;
    settings.large_contributor.share =
        (rule(dto, "contextDominance")?.threshold(25.0)? / 100.0) as f32;
    settings.residual_step.delivery = rule(dto, "residualStep")?.delivery()?;
    settings.residual_step.tokens = whole_threshold(rule(dto, "residualStep")?, 5_000)?;
    settings.instruction_drift.delivery = rule(dto, "instructionDrift")?.delivery()?;
    let waste = rule(dto, "contextWaste")?;
    settings.duplicate_context.delivery = waste.delivery()?;
    settings.low_entropy_content.delivery = waste.delivery()?;
    let waste_tokens = whole_threshold(waste, 10_000)?;
    settings.duplicate_context.tokens = waste_tokens;
    settings.low_entropy_content.tokens = waste_tokens;
    settings.cost_budget.delivery = rule(dto, "costBudget")?.delivery()?;
    settings.cost_budget_micros = dto
        .cost_budget_usd
        .map(|value| {
            if !value.is_finite() || value < 0.0 {
                return Err("costBudgetUsd must be a non-negative finite number".to_string());
            }
            Ok((value * 1_000_000.0).round().min(u64::MAX as f64) as u64)
        })
        .transpose()?;
    Ok(settings)
}

impl RuleSettingDto {
    fn delivery(&self) -> Result<NotificationDelivery, String> {
        match self.delivery.as_str() {
            "off" => Ok(NotificationDelivery::Off),
            "feed" => Ok(NotificationDelivery::FeedOnly),
            "feedAndOs" => Ok(NotificationDelivery::FeedAndOs),
            _ => Err(format!("unknown notification delivery '{}'", self.delivery)),
        }
    }

    fn threshold(&self, fallback: f64) -> Result<f64, String> {
        let value = self.threshold.unwrap_or(fallback);
        if value.is_finite() && value >= 0.0 {
            Ok(value)
        } else {
            Err("notification thresholds must be non-negative finite numbers".into())
        }
    }
}

fn rule<'a>(dto: &'a NotificationSettingsDto, id: &str) -> Result<&'a RuleSettingDto, String> {
    dto.rules
        .get(id)
        .ok_or_else(|| format!("missing notification rule '{id}'"))
}

fn whole_threshold(setting: &RuleSettingDto, fallback: u32) -> Result<u32, String> {
    Ok(setting
        .threshold(f64::from(fallback))?
        .round()
        .min(f64::from(u32::MAX)) as u32)
}

fn domain_has_os_delivery(settings: &NotificationSettings) -> bool {
    [
        settings.context_pressure.delivery,
        settings.prompt_spike.delivery,
        settings.large_contributor.delivery,
        settings.tool_error_streak.delivery,
        settings.compaction.delivery,
        settings.secret_exposure.delivery,
        settings.format_drift.delivery,
        settings.duplicate_context.delivery,
        settings.low_entropy_content.delivery,
        settings.residual_step.delivery,
        settings.instruction_drift.delivery,
        settings.cost_budget.delivery,
    ]
    .into_iter()
    .any(NotificationDelivery::includes_os)
}

fn record_dto(record: &NotificationRecord) -> NotificationRecordDto {
    let detected_at = timestamp(record.detected_at_ms);
    NotificationRecordDto {
        id: record.id.to_string(),
        rule_id: rule_label(record.candidate.rule).into(),
        severity: severity_label(record.candidate.severity).into(),
        delivery: delivery_label(record.candidate.delivery).into(),
        confidence: confidence_label(record.candidate.rule).into(),
        title: record.candidate.title.clone(),
        description: record.candidate.body.clone(),
        occurred_at: timestamp(
            record
                .candidate
                .occurred_at_ms
                .unwrap_or(record.detected_at_ms),
        ),
        detected_at,
        read_at: record.read_at_ms.map(timestamp),
        dismissed_at: record.dismissed_at_ms.map(timestamp),
        catch_up: record.catch_up,
        os_delivery: OsDeliveryDto::from(&record.os_delivery),
        location: NotificationLocationDto {
            agent: record.candidate.location.agent.to_string(),
            session_id: record.candidate.location.session_id.to_string(),
            project: record.candidate.location.project.clone(),
            turn: record.candidate.location.turn.map(|turn| turn.get()),
            source_line: record.candidate.location.line,
        },
    }
}

fn timestamp(milliseconds: u64) -> String {
    DateTime::<Utc>::from(UNIX_EPOCH + Duration::from_millis(milliseconds)).to_rfc3339()
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u128::from(u64::MAX)) as u64
}

fn rule_label(rule: NotificationRuleId) -> &'static str {
    match rule {
        NotificationRuleId::ContextPressure => "contextPressure",
        NotificationRuleId::PromptSpike => "promptSpike",
        NotificationRuleId::ToolErrorStreak => "toolErrorStreak",
        NotificationRuleId::SecretExposure => "secretExposure",
        NotificationRuleId::FormatDrift => "formatDrift",
        NotificationRuleId::Compaction => "compaction",
        NotificationRuleId::LargeContributor => "contextDominance",
        NotificationRuleId::ResidualStep => "residualStep",
        NotificationRuleId::InstructionDrift => "instructionDrift",
        NotificationRuleId::DuplicateContext | NotificationRuleId::LowEntropyContent => {
            "contextWaste"
        }
        NotificationRuleId::CostBudget => "costBudget",
    }
}

fn severity_label(severity: ct_domain::NotificationSeverity) -> &'static str {
    match severity {
        ct_domain::NotificationSeverity::Info => "info",
        ct_domain::NotificationSeverity::Warning => "warning",
        ct_domain::NotificationSeverity::Critical => "critical",
    }
}

fn delivery_label(delivery: NotificationDelivery) -> &'static str {
    match delivery {
        NotificationDelivery::Off => "off",
        NotificationDelivery::FeedOnly => "feed",
        NotificationDelivery::FeedAndOs => "feedAndOs",
    }
}

fn confidence_label(rule: NotificationRuleId) -> &'static str {
    match rule {
        NotificationRuleId::ResidualStep | NotificationRuleId::CostBudget => "derived",
        NotificationRuleId::LargeContributor
        | NotificationRuleId::DuplicateContext
        | NotificationRuleId::LowEntropyContent => "estimated",
        _ => "observed",
    }
}

fn permission_label(result: Result<PermissionState, impl std::fmt::Display>) -> &'static str {
    match result {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Denied) => "denied",
        Ok(PermissionState::Prompt) => "prompt",
        Ok(_) | Err(_) => "unsupported",
    }
}

/// Send one record's toast and report what Windows did with it.
///
/// The permission check stays first for the platforms where it means
/// something. On Windows the plugin answers `Granted` unconditionally, so the
/// check passing there says nothing at all -- which is why
/// [`delivery::deliver`] re-asks the question the platform can actually answer
/// before it sends.
fn deliver_os(
    app: &AppHandle,
    state: &NotificationState,
    record: &NotificationRecord,
) -> OsDeliveryStatus {
    match app.notification().permission_state() {
        Ok(PermissionState::Granted) => delivery::deliver(
            state.deliverability(app),
            &record.candidate.title,
            &record.candidate.body,
        ),
        Ok(permission) => OsDeliveryStatus::Failed {
            reason: format!("OS notification permission is {permission:?}"),
        },
        Err(error) => OsDeliveryStatus::Failed {
            reason: error.to_string(),
        },
    }
}

fn parse_ids(ids: Option<Vec<String>>) -> Result<Option<Vec<u64>>, String> {
    ids.map(|ids| {
        ids.into_iter()
            .map(|id| {
                id.parse()
                    .map_err(|_| format!("invalid notification id '{id}'"))
            })
            .collect()
    })
    .transpose()
}

fn emit_updated_records(
    app: &AppHandle,
    state: &NotificationState,
    ids: Option<&[u64]>,
) -> Result<(), String> {
    let records = state
        .store
        .records(None, usize::MAX)
        .map_err(|error| error.to_string())?;
    for record in records
        .iter()
        .filter(|record| ids.is_none_or(|ids| ids.contains(&record.id)))
    {
        app.emit("contexttrace://notification-updated", record_dto(record))
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct SessionUpdatedEvent {
    agent: String,
    session_id: String,
}

fn subagent_os_allowed(
    role: &ThreadRole,
    preferences: ct_runtime::NotificationUiPreferences,
) -> bool {
    !matches!(role, ThreadRole::Subagent { .. }) || preferences.subagent_os_notifications
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domain_defaults_present_the_complete_frontend_contract() {
        let dto = domain_to_settings(
            &NotificationSettings::default(),
            ct_runtime::NotificationUiPreferences::default(),
        );
        assert!(!dto.enabled);
        assert!(!dto.onboarding_complete);
        assert_eq!(dto.rules.len(), 11);
        assert_eq!(dto.rules["contextPressure"].threshold, Some(75.0));
        assert_eq!(dto.rules["promptSpike"].threshold, Some(20_000.0));
        assert_eq!(dto.rules["contextWaste"].delivery, "feed");
        assert_eq!(dto.rules["costBudget"].delivery, "off");
    }

    #[test]
    fn frontend_thresholds_round_trip_into_typed_domain_rules() {
        let mut dto = domain_to_settings(
            &NotificationSettings::default(),
            ct_runtime::NotificationUiPreferences::default(),
        );
        dto.enabled = true;
        dto.cost_budget_usd = Some(1.25);
        dto.rules.get_mut("contextPressure").unwrap().threshold = Some(82.0);
        dto.rules.get_mut("contextWaste").unwrap().threshold = Some(12_345.0);
        dto.rules.get_mut("costBudget").unwrap().delivery = "feedAndOs".into();

        let domain = settings_to_domain(&dto, NotificationSettings::default()).unwrap();

        assert!(domain.enabled);
        assert!((domain.context_pressure.warning - 0.82).abs() < f32::EPSILON);
        assert_eq!(domain.duplicate_context.tokens, 12_345);
        assert_eq!(domain.low_entropy_content.tokens, 12_345);
        assert_eq!(domain.cost_budget_micros, Some(1_250_000));
        assert_eq!(domain.cost_budget.delivery, NotificationDelivery::FeedAndOs);
    }
}
