//! Durable notification vocabulary shared by rule evaluators, stores and UIs.

use super::identity::{SessionId, TurnNumber};
use super::session::AgentKind;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationRuleId {
    ContextPressure,
    PromptSpike,
    LargeContributor,
    ToolErrorStreak,
    Compaction,
    SecretExposure,
    FormatDrift,
    DuplicateContext,
    LowEntropyContent,
    ResidualStep,
    InstructionDrift,
    CostBudget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationSeverity {
    Info,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NotificationDelivery {
    Off,
    FeedOnly,
    FeedAndOs,
}

impl NotificationDelivery {
    pub fn enabled(self) -> bool {
        self != Self::Off
    }
    pub fn includes_os(self) -> bool {
        self == Self::FeedAndOs
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum PressureBand {
    #[default]
    Normal,
    Warning,
    Critical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRuleSettings {
    pub delivery: NotificationDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextPressureSettings {
    pub delivery: NotificationDelivery,
    pub warning: f32,
    pub critical: f32,
    pub reset_below_warning: f32,
    pub reset_below_critical: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenThresholdSettings {
    pub delivery: NotificationDelivery,
    pub tokens: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CountThresholdSettings {
    pub delivery: NotificationDelivery,
    pub count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContributorThresholdSettings {
    pub delivery: NotificationDelivery,
    pub tokens: u32,
    pub share: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    pub enabled: bool,
    pub include_subagents: bool,
    pub context_pressure: ContextPressureSettings,
    pub prompt_spike: TokenThresholdSettings,
    pub large_contributor: ContributorThresholdSettings,
    pub tool_error_streak: CountThresholdSettings,
    pub compaction: DeliveryRuleSettings,
    pub secret_exposure: DeliveryRuleSettings,
    pub format_drift: DeliveryRuleSettings,
    pub duplicate_context: TokenThresholdSettings,
    pub low_entropy_content: TokenThresholdSettings,
    pub residual_step: TokenThresholdSettings,
    pub instruction_drift: DeliveryRuleSettings,
    /// Session cost in millionths of the configured currency unit.
    pub cost_budget_micros: Option<u64>,
    pub cost_budget: DeliveryRuleSettings,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        use NotificationDelivery::{FeedAndOs, FeedOnly};
        Self {
            enabled: false,
            include_subagents: true,
            context_pressure: ContextPressureSettings {
                delivery: FeedAndOs,
                warning: 0.75,
                critical: 0.90,
                reset_below_warning: 0.70,
                reset_below_critical: 0.85,
            },
            prompt_spike: TokenThresholdSettings {
                delivery: FeedAndOs,
                tokens: 20_000,
            },
            large_contributor: ContributorThresholdSettings {
                delivery: FeedOnly,
                tokens: 20_000,
                share: 0.25,
            },
            tool_error_streak: CountThresholdSettings {
                delivery: FeedAndOs,
                count: 3,
            },
            compaction: DeliveryRuleSettings { delivery: FeedOnly },
            secret_exposure: DeliveryRuleSettings {
                delivery: FeedAndOs,
            },
            format_drift: DeliveryRuleSettings {
                delivery: FeedAndOs,
            },
            duplicate_context: TokenThresholdSettings {
                delivery: FeedOnly,
                tokens: 10_000,
            },
            low_entropy_content: TokenThresholdSettings {
                delivery: FeedOnly,
                tokens: 20_000,
            },
            residual_step: TokenThresholdSettings {
                delivery: FeedAndOs,
                tokens: 5_000,
            },
            instruction_drift: DeliveryRuleSettings {
                delivery: FeedAndOs,
            },
            cost_budget_micros: None,
            cost_budget: DeliveryRuleSettings {
                delivery: FeedAndOs,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationLocation {
    pub agent: AgentKind,
    pub session_id: SessionId,
    pub project: Option<String>,
    pub turn: Option<TurnNumber>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum NotificationEvidence {
    ContextPressure {
        prompt_tokens: u32,
        context_window: u32,
        utilisation: f32,
        band: PressureBand,
    },
    PromptSpike {
        previous_tokens: u32,
        tokens: u32,
        growth: u32,
        candidates: Vec<String>,
    },
    LargeContributor {
        item_id: String,
        label: String,
        tokens: u32,
        share: f32,
    },
    ToolErrorStreak {
        tool: String,
        streak: u32,
    },
    Compaction {
        trigger: Option<String>,
        tokens_before: Option<u32>,
        tokens_after: Option<u32>,
        reclaimed: Option<u32>,
    },
    SecretExposure {
        secret_kind: String,
        occurrences: usize,
    },
    FormatDrift {
        raw_type: String,
        events: u32,
        fidelity: f32,
    },
    DuplicateContext {
        copies: usize,
        repeated_tokens: u32,
        share: f32,
    },
    LowEntropyContent {
        item_id: String,
        label: String,
        waste_score_tokens: u32,
        compression_ratio: f32,
    },
    ResidualStep {
        from: u32,
        to: u32,
        growth: i64,
    },
    InstructionDrift {
        mechanism: String,
        from_label: String,
        to_label: String,
    },
    CostBudget {
        observed_micros: u64,
        projected_micros: Option<u64>,
        budget_micros: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationCandidate {
    pub dedupe_key: String,
    pub rule: NotificationRuleId,
    pub severity: NotificationSeverity,
    pub delivery: NotificationDelivery,
    pub title: String,
    pub body: String,
    pub location: NotificationLocation,
    /// When the underlying session event happened, if the agent recorded it.
    pub occurred_at_ms: Option<u64>,
    pub evidence: NotificationEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum OsDeliveryStatus {
    NotRequested,
    Delivered,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationRecord {
    pub id: u64,
    pub detected_at_ms: u64,
    pub catch_up: bool,
    pub candidate: NotificationCandidate,
    pub read_at_ms: Option<u64>,
    pub dismissed_at_ms: Option<u64>,
    pub os_delivery: OsDeliveryStatus,
}

impl NotificationRecord {
    pub fn is_read(&self) -> bool {
        self.read_at_ms.is_some()
    }
    pub fn is_dismissed(&self) -> bool {
        self.dismissed_at_ms.is_some()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFingerprint {
    pub path: String,
    pub size_bytes: u64,
    pub last_activity: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNotificationCheckpoint {
    pub agent: AgentKind,
    pub session_id: SessionId,
    pub fingerprint: Option<SessionFingerprint>,
    pub last_sequence: Option<u32>,
    pub last_turn: Option<TurnNumber>,
    pub pressure_band: PressureBand,
    pub error_tool: Option<String>,
    pub consecutive_tool_errors: u32,
}

impl SessionNotificationCheckpoint {
    pub fn baseline(agent: AgentKind, session_id: SessionId) -> Self {
        Self {
            agent,
            session_id,
            fingerprint: None,
            last_sequence: None,
            last_turn: None,
            pressure_band: PressureBand::Normal,
            error_tool: None,
            consecutive_tool_errors: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_noisy_findings_in_the_feed() {
        let settings = NotificationSettings::default();
        assert!(!settings.enabled, "monitoring requires explicit opt-in");
        assert_eq!(settings.context_pressure.warning, 0.75);
        assert_eq!(settings.context_pressure.critical, 0.90);
        assert_eq!(settings.compaction.delivery, NotificationDelivery::FeedOnly);
        assert!(settings.secret_exposure.delivery.includes_os());
        assert_eq!(settings.cost_budget_micros, None);
    }

    #[test]
    fn a_new_checkpoint_contains_no_invented_cursor() {
        let checkpoint = SessionNotificationCheckpoint::baseline(
            AgentKind::Codex,
            SessionId::new("session").unwrap(),
        );
        assert_eq!(checkpoint.last_sequence, None);
        assert_eq!(checkpoint.last_turn, None);
        assert_eq!(checkpoint.pressure_band, PressureBand::Normal);
    }
}
