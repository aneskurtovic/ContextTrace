//! Pure notification rule evaluation over parsed sessions and optional deep analyses.

use crate::{InstructionDrift, ResidualStep, SecretFinding};
use ct_domain::{
    AgentSession, ContextSnapshot, EventKind, NotificationCandidate, NotificationDelivery,
    NotificationEvidence, NotificationLocation, NotificationRuleId, NotificationSettings,
    NotificationSeverity, PressureBand, SessionNotificationCheckpoint, TurnNumber,
};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CostBudgetObservation {
    pub observed_micros: u64,
    pub projected_micros: Option<u64>,
}

#[derive(Default)]
pub struct NotificationInputs<'a> {
    pub snapshots: &'a [ContextSnapshot],
    pub secret_findings: &'a [SecretFinding],
    pub residual_steps: &'a [ResidualStep],
    pub instruction_drift: Option<&'a InstructionDrift>,
    pub cost: Option<CostBudgetObservation>,
}

#[derive(Debug, Clone)]
pub struct NotificationEvaluation {
    pub candidates: Vec<NotificationCandidate>,
    pub checkpoint: SessionNotificationCheckpoint,
}

pub struct NotificationEngine;

impl NotificationEngine {
    /// Establish a no-notification cursor for sessions present before monitoring starts.
    pub fn baseline(session: &AgentSession) -> SessionNotificationCheckpoint {
        Self::baseline_with_settings(session, &NotificationSettings::default())
    }

    /// Establish a no-alert cursor using the active thresholds. Callers should
    /// prefer this when notification settings have already been loaded.
    pub fn baseline_with_settings(
        session: &AgentSession,
        settings: &NotificationSettings,
    ) -> SessionNotificationCheckpoint {
        let mut checkpoint =
            SessionNotificationCheckpoint::baseline(session.agent(), session.id().clone());
        checkpoint.last_sequence = session.events().last().map(|event| event.sequence);
        checkpoint.last_turn = session.turns().last().map(|turn| turn.number);
        checkpoint.pressure_band = session
            .turns()
            .last()
            .and_then(|turn| utilisation(session, turn.number))
            .map(|value| raw_pressure_band(value, settings))
            .unwrap_or_default();
        checkpoint
    }

    pub fn evaluate(
        session: &AgentSession,
        prior: &SessionNotificationCheckpoint,
        settings: &NotificationSettings,
        inputs: NotificationInputs<'_>,
    ) -> NotificationEvaluation {
        let mut checkpoint = prior.clone();
        let mut candidates = Vec::new();
        if !settings.enabled {
            advance(session, &mut checkpoint);
            return NotificationEvaluation {
                candidates,
                checkpoint,
            };
        }

        evaluate_pressure(session, prior, settings, &mut checkpoint, &mut candidates);
        evaluate_spikes(session, prior, settings, &mut candidates);
        evaluate_events(session, prior, settings, &mut checkpoint, &mut candidates);
        evaluate_deep(session, settings, inputs, &mut candidates);
        advance(session, &mut checkpoint);
        NotificationEvaluation {
            candidates,
            checkpoint,
        }
    }
}

fn location(
    session: &AgentSession,
    turn: Option<TurnNumber>,
    line: Option<u32>,
) -> NotificationLocation {
    NotificationLocation {
        agent: session.agent(),
        session_id: session.id().clone(),
        project: session.metadata().project.clone(),
        turn,
        line,
    }
}

// Keeping these fields named at each rule call site makes privacy-sensitive
// notification copy and evidence reviewable together. A positional builder
// would only move the same arguments into a less direct intermediate shape.
#[allow(clippy::too_many_arguments)]
fn candidate(
    session: &AgentSession,
    key: String,
    rule: NotificationRuleId,
    severity: NotificationSeverity,
    delivery: NotificationDelivery,
    title: &str,
    body: String,
    turn: Option<TurnNumber>,
    line: Option<u32>,
    evidence: NotificationEvidence,
) -> NotificationCandidate {
    let occurred_at_ms = turn
        .and_then(|number| session.turn(number))
        .and_then(|turn| turn.timestamp.as_ref())
        .or_else(|| {
            line.and_then(|line| {
                session
                    .events()
                    .iter()
                    .find(|event| event.source.line_no == line)
            })
            .and_then(|event| event.timestamp.as_ref())
        })
        .and_then(|time| u64::try_from(time.timestamp_millis()).ok());
    NotificationCandidate {
        dedupe_key: key,
        rule,
        severity,
        delivery,
        title: title.into(),
        body,
        location: location(session, turn, line),
        occurred_at_ms,
        evidence,
    }
}

fn advance(session: &AgentSession, checkpoint: &mut SessionNotificationCheckpoint) {
    checkpoint.last_sequence = session
        .events()
        .last()
        .map(|event| event.sequence)
        .or(checkpoint.last_sequence);
    checkpoint.last_turn = session
        .turns()
        .last()
        .map(|turn| turn.number)
        .or(checkpoint.last_turn);
}

fn utilisation(session: &AgentSession, turn: TurnNumber) -> Option<f32> {
    let turn = session.turn(turn)?;
    let used = turn.prompt_tokens()?;
    let window = turn
        .usage
        .context_window
        .or(session.metadata().context_window)?;
    (window > 0).then(|| used as f32 / window as f32)
}

fn raw_pressure_band(value: f32, settings: &NotificationSettings) -> PressureBand {
    if value >= settings.context_pressure.critical {
        PressureBand::Critical
    } else if value >= settings.context_pressure.warning {
        PressureBand::Warning
    } else {
        PressureBand::Normal
    }
}

fn next_pressure_band(
    previous: PressureBand,
    value: f32,
    settings: &NotificationSettings,
) -> PressureBand {
    let pressure = &settings.context_pressure;
    if value >= pressure.critical {
        return PressureBand::Critical;
    }
    if previous == PressureBand::Critical && value >= pressure.reset_below_critical {
        return PressureBand::Critical;
    }
    if value >= pressure.warning {
        return PressureBand::Warning;
    }
    if previous == PressureBand::Warning && value >= pressure.reset_below_warning {
        return PressureBand::Warning;
    }
    PressureBand::Normal
}

fn evaluate_pressure(
    session: &AgentSession,
    prior: &SessionNotificationCheckpoint,
    settings: &NotificationSettings,
    checkpoint: &mut SessionNotificationCheckpoint,
    out: &mut Vec<NotificationCandidate>,
) {
    for turn in session
        .turns()
        .iter()
        .filter(|t| prior.last_turn.is_none_or(|last| t.number > last))
    {
        evaluate_pressure_turn(session, turn, settings, checkpoint, out);
    }
}

fn evaluate_pressure_turn(
    session: &AgentSession,
    turn: &ct_domain::Turn,
    settings: &NotificationSettings,
    checkpoint: &mut SessionNotificationCheckpoint,
    out: &mut Vec<NotificationCandidate>,
) {
    let Some(value) = utilisation(session, turn.number) else {
        return;
    };
    let next = next_pressure_band(checkpoint.pressure_band, value, settings);
    notify_pressure(
        session,
        turn,
        value,
        next,
        checkpoint.pressure_band,
        settings,
        out,
    );
    checkpoint.pressure_band = next;
}

fn notify_pressure(
    session: &AgentSession,
    turn: &ct_domain::Turn,
    value: f32,
    next: PressureBand,
    previous: PressureBand,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let delivery = settings.context_pressure.delivery;
    if !delivery.enabled() || next <= previous {
        return;
    }
    let severity = match next {
        PressureBand::Critical => NotificationSeverity::Critical,
        _ => NotificationSeverity::Warning,
    };
    let used = turn.prompt_tokens().unwrap_or_default();
    let window = turn
        .usage
        .context_window
        .or(session.metadata().context_window)
        .unwrap_or_default();
    let evidence = NotificationEvidence::ContextPressure {
        prompt_tokens: used,
        context_window: window,
        utilisation: value,
        band: next,
    };
    push_pressure(
        session,
        turn.number,
        value,
        severity,
        delivery,
        evidence,
        out,
    );
}

fn push_pressure(
    session: &AgentSession,
    turn: TurnNumber,
    value: f32,
    severity: NotificationSeverity,
    delivery: NotificationDelivery,
    evidence: NotificationEvidence,
    out: &mut Vec<NotificationCandidate>,
) {
    out.push(candidate(
        session,
        format!("pressure:{}:{}", session.id(), turn),
        NotificationRuleId::ContextPressure,
        severity,
        delivery,
        "Context window pressure",
        format!(
            "Turn {turn} uses {:.0}% of its context window.",
            value * 100.0
        ),
        Some(turn),
        None,
        evidence,
    ));
}

fn evaluate_spikes(
    session: &AgentSession,
    prior: &SessionNotificationCheckpoint,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let rule = settings.prompt_spike;
    if !rule.delivery.enabled() {
        return;
    }
    for pair in session.turns().windows(2) {
        let (before, after) = (&pair[0], &pair[1]);
        if prior.last_turn.is_some_and(|last| after.number <= last) {
            continue;
        }
        let (Some(from), Some(to)) = (before.prompt_tokens(), after.prompt_tokens()) else {
            continue;
        };
        if from == 0 || to <= from || to - from < rule.tokens {
            continue;
        }
        push_spike(session, after.number, from, to, rule.delivery, out);
    }
}

fn push_spike(
    session: &AgentSession,
    turn: TurnNumber,
    from: u32,
    to: u32,
    delivery: NotificationDelivery,
    out: &mut Vec<NotificationCandidate>,
) {
    let growth = to - from;
    let evidence = NotificationEvidence::PromptSpike {
        previous_tokens: from,
        tokens: to,
        growth,
        candidates: Vec::new(),
    };
    out.push(candidate(
        session,
        format!("spike:{}:{}", session.id(), turn),
        NotificationRuleId::PromptSpike,
        NotificationSeverity::Warning,
        delivery,
        "Prompt size jumped",
        format!("Turn {turn} added {growth} prompt tokens."),
        Some(turn),
        None,
        evidence,
    ));
}

fn evaluate_events(
    session: &AgentSession,
    prior: &SessionNotificationCheckpoint,
    settings: &NotificationSettings,
    checkpoint: &mut SessionNotificationCheckpoint,
    out: &mut Vec<NotificationCandidate>,
) {
    let tools: HashMap<&str, (&str, Option<&str>)> = session
        .events()
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::ToolCall {
                tool,
                call_id: Some(id),
                target,
                ..
            } => Some((id.as_str(), (tool.as_str(), target.as_deref()))),
            _ => None,
        })
        .collect();
    let mut unknown: HashMap<&str, u32> = HashMap::new();
    for event in session
        .events()
        .iter()
        .filter(|event| prior.last_sequence.is_none_or(|last| event.sequence > last))
    {
        match &event.kind {
            EventKind::ToolResult {
                tool,
                call_id,
                is_error,
                ..
            } => {
                let call = call_id.as_deref().and_then(|id| tools.get(id).copied());
                let display_tool = tool.as_deref().or_else(|| call.map(|(tool, _)| tool));
                let failure_key = display_tool
                    .map(|tool| tool_failure_key(tool, call.and_then(|(_, target)| target)));
                evaluate_tool_result(
                    session,
                    event,
                    display_tool,
                    failure_key.as_deref(),
                    *is_error,
                    settings,
                    checkpoint,
                    out,
                )
            }
            EventKind::Compacted(facts) => push_compaction(session, event, facts, settings, out),
            EventKind::Unrecognised => *unknown.entry(event.raw_type.as_str()).or_default() += 1,
            _ => {}
        }
    }
    for (raw_type, count) in unknown {
        push_format_drift(session, raw_type, count, settings, out);
    }
}

// Keeping the event and rule inputs explicit makes this detector auditable and avoids
// retaining sensitive tool targets in a shared intermediate structure.
#[allow(clippy::too_many_arguments)]
fn evaluate_tool_result(
    session: &AgentSession,
    event: &ct_domain::Event,
    tool: Option<&str>,
    failure_key: Option<&str>,
    failed: bool,
    settings: &NotificationSettings,
    checkpoint: &mut SessionNotificationCheckpoint,
    out: &mut Vec<NotificationCandidate>,
) {
    if !failed {
        checkpoint.error_tool = None;
        checkpoint.consecutive_tool_errors = 0;
        return;
    }
    let tool = tool.unwrap_or("tool");
    let failure_key = failure_key.unwrap_or(tool);
    if checkpoint.error_tool.as_deref() == Some(failure_key) {
        checkpoint.consecutive_tool_errors += 1;
    } else {
        checkpoint.error_tool = Some(failure_key.into());
        checkpoint.consecutive_tool_errors = 1;
    }
    let rule = settings.tool_error_streak;
    if rule.delivery.enabled() && checkpoint.consecutive_tool_errors == rule.count.max(1) {
        let streak = checkpoint.consecutive_tool_errors;
        let evidence = NotificationEvidence::ToolErrorStreak {
            tool: tool.into(),
            streak,
        };
        out.push(candidate(
            session,
            format!("tool-errors:{}:{}", session.id(), event.sequence),
            NotificationRuleId::ToolErrorStreak,
            NotificationSeverity::Warning,
            rule.delivery,
            "Tool failures are repeating",
            format!("{tool} failed {streak} times in a row."),
            event.turn,
            Some(event.source.line_no),
            evidence,
        ));
    }
}

fn tool_failure_key(tool: &str, target: Option<&str>) -> String {
    let Some(target) = target else {
        return tool.to_string();
    };
    // FNV-1a is sufficient here: this is a stable, non-reversible grouping key,
    // not a security boundary. Persisting the target itself would leak commands
    // and full paths into notification checkpoints.
    let mut hash = 0xcbf29ce484222325u64;
    for byte in target.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{tool}:{hash:016x}")
}

fn push_compaction(
    session: &AgentSession,
    event: &ct_domain::Event,
    facts: &ct_domain::model::event::CompactionFacts,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let delivery = settings.compaction.delivery;
    if !delivery.enabled() {
        return;
    }
    let reclaimed = facts
        .tokens_before
        .zip(facts.tokens_after)
        .map(|(a, b)| a.saturating_sub(b));
    let evidence = NotificationEvidence::Compaction {
        trigger: facts.trigger.clone(),
        tokens_before: facts.tokens_before,
        tokens_after: facts.tokens_after,
        reclaimed,
    };
    let body = reclaimed
        .map(|n| format!("Compaction reclaimed {n} tokens."))
        .unwrap_or_else(|| "The agent compacted its context.".into());
    out.push(candidate(
        session,
        format!("compaction:{}:{}", session.id(), event.source.line_no),
        NotificationRuleId::Compaction,
        NotificationSeverity::Info,
        delivery,
        "Context compacted",
        body,
        event.turn,
        Some(event.source.line_no),
        evidence,
    ));
}

fn push_format_drift(
    session: &AgentSession,
    raw_type: &str,
    count: u32,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let delivery = settings.format_drift.delivery;
    if !delivery.enabled() {
        return;
    }
    let evidence = NotificationEvidence::FormatDrift {
        raw_type: raw_type.into(),
        events: count,
        fidelity: session.fidelity(),
    };
    out.push(candidate(
        session,
        format!("format:{}:{raw_type}", session.agent()),
        NotificationRuleId::FormatDrift,
        NotificationSeverity::Warning,
        delivery,
        "Agent log format changed",
        format!("{count} new {raw_type} event(s) were not recognised."),
        None,
        None,
        evidence,
    ));
}

fn evaluate_deep(
    session: &AgentSession,
    settings: &NotificationSettings,
    inputs: NotificationInputs<'_>,
    out: &mut Vec<NotificationCandidate>,
) {
    for snapshot in inputs.snapshots {
        evaluate_snapshot(session, snapshot, settings, out);
    }
    for finding in inputs.secret_findings {
        push_secret(session, finding, settings, out);
    }
    for step in inputs.residual_steps {
        push_residual(session, step, settings, out);
    }
    if let Some(drift) = inputs.instruction_drift {
        for change in &drift.changes {
            push_instruction(session, change, settings, out);
        }
    }
    if let Some(cost) = inputs.cost {
        push_cost(session, cost, settings, out);
    }
}

fn evaluate_snapshot(
    session: &AgentSession,
    snapshot: &ContextSnapshot,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let rule = settings.large_contributor;
    if rule.delivery.enabled() {
        for item in snapshot
            .largest_contributors(20)
            .into_iter()
            .filter(|item| item.tokens >= rule.tokens || item.share >= rule.share)
        {
            let evidence = NotificationEvidence::LargeContributor {
                item_id: item.id.to_string(),
                label: short_label(&item.label).into(),
                tokens: item.tokens,
                share: item.share,
            };
            out.push(candidate(
                session,
                format!(
                    "contributor:{}:{}:{}",
                    session.id(),
                    snapshot.turn(),
                    item.id
                ),
                NotificationRuleId::LargeContributor,
                NotificationSeverity::Warning,
                rule.delivery,
                "Large context contributor",
                format!(
                    "{} occupies {:.0}% of turn {}.",
                    short_label(&item.label),
                    item.share * 100.0,
                    snapshot.turn()
                ),
                Some(snapshot.turn()),
                None,
                evidence,
            ));
        }
    }
    evaluate_waste(session, snapshot, settings, out);
}

fn short_label(label: &str) -> &str {
    label.rsplit(['/', '\\']).next().unwrap_or(label)
}

fn evaluate_waste(
    session: &AgentSession,
    snapshot: &ContextSnapshot,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let duplicate = settings.duplicate_context;
    if duplicate.delivery.enabled() {
        for group in snapshot
            .duplicate_content()
            .into_iter()
            .filter(|g| g.repeated_tokens >= duplicate.tokens)
        {
            let ids = group
                .items
                .iter()
                .map(|item| item.id.to_string())
                .collect::<Vec<_>>()
                .join(",");
            let evidence = NotificationEvidence::DuplicateContext {
                copies: group.items.len(),
                repeated_tokens: group.repeated_tokens,
                share: group.share,
            };
            out.push(candidate(
                session,
                format!("duplicate:{}:{}:{ids}", session.id(), snapshot.turn()),
                NotificationRuleId::DuplicateContext,
                NotificationSeverity::Warning,
                duplicate.delivery,
                "Duplicate context detected",
                format!(
                    "{} repeated tokens occupy this turn.",
                    group.repeated_tokens
                ),
                Some(snapshot.turn()),
                None,
                evidence,
            ));
        }
    }
    let entropy = settings.low_entropy_content;
    if entropy.delivery.enabled() {
        for item in snapshot
            .low_entropy_content()
            .into_iter()
            .filter(|item| item.waste_score_tokens >= entropy.tokens)
        {
            let evidence = NotificationEvidence::LowEntropyContent {
                item_id: item.id.to_string(),
                label: short_label(&item.label).into(),
                waste_score_tokens: item.waste_score_tokens,
                compression_ratio: item.compression_ratio,
            };
            out.push(candidate(
                session,
                format!("entropy:{}:{}:{}", session.id(), snapshot.turn(), item.id),
                NotificationRuleId::LowEntropyContent,
                NotificationSeverity::Warning,
                entropy.delivery,
                "Low-information context",
                format!(
                    "{} has a {} token waste score.",
                    short_label(&item.label),
                    item.waste_score_tokens
                ),
                Some(snapshot.turn()),
                None,
                evidence,
            ));
        }
    }
}

fn push_secret(
    session: &AgentSession,
    finding: &SecretFinding,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let delivery = settings.secret_exposure.delivery;
    if !delivery.enabled() {
        return;
    }
    let kind = finding.kind.label();
    let evidence = NotificationEvidence::SecretExposure {
        secret_kind: kind.into(),
        occurrences: finding.occurrences,
    };
    out.push(candidate(
        session,
        format!(
            "secret:{}:{}:{}",
            session.id(),
            finding.line_no,
            finding.kind.marker()
        ),
        NotificationRuleId::SecretExposure,
        NotificationSeverity::Critical,
        delivery,
        "Secret entered model context",
        format!("{kind} detected at line {}.", finding.line_no),
        finding.turn,
        Some(finding.line_no),
        evidence,
    ));
}

fn push_residual(
    session: &AgentSession,
    step: &ResidualStep,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let rule = settings.residual_step;
    if !rule.delivery.enabled() || step.growth().unsigned_abs() < u64::from(rule.tokens) {
        return;
    }
    let Some(turn) = TurnNumber::new(step.turn).ok() else {
        return;
    };
    let evidence = NotificationEvidence::ResidualStep {
        from: step.from,
        to: step.to,
        growth: step.growth(),
    };
    out.push(candidate(
        session,
        format!("residual:{}:{}", session.id(), step.turn),
        NotificationRuleId::ResidualStep,
        NotificationSeverity::Warning,
        rule.delivery,
        "Hidden context changed",
        format!("Unattributed context changed by {} tokens.", step.growth()),
        Some(turn),
        None,
        evidence,
    ));
}

fn push_instruction(
    session: &AgentSession,
    change: &crate::InstructionChange,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let delivery = settings.instruction_drift.delivery;
    if !delivery.enabled() {
        return;
    }
    let turn = change.to_turn.and_then(|turn| TurnNumber::new(turn).ok());
    let evidence = NotificationEvidence::InstructionDrift {
        mechanism: change.mechanism.clone(),
        from_label: short_label(&change.from_label).into(),
        to_label: short_label(&change.to_label).into(),
    };
    out.push(candidate(
        session,
        format!(
            "instructions:{}:{}:{:?}",
            session.id(),
            change.mechanism,
            change.to_turn
        ),
        NotificationRuleId::InstructionDrift,
        NotificationSeverity::Warning,
        delivery,
        "Instructions changed",
        format!(
            "{} instructions changed during the session.",
            change.mechanism
        ),
        turn,
        None,
        evidence,
    ));
}

fn push_cost(
    session: &AgentSession,
    cost: CostBudgetObservation,
    settings: &NotificationSettings,
    out: &mut Vec<NotificationCandidate>,
) {
    let (Some(budget), delivery) = (settings.cost_budget_micros, settings.cost_budget.delivery)
    else {
        return;
    };
    let compared = cost.projected_micros.unwrap_or(cost.observed_micros);
    if !delivery.enabled() || compared < budget {
        return;
    }
    let evidence = NotificationEvidence::CostBudget {
        observed_micros: cost.observed_micros,
        projected_micros: cost.projected_micros,
        budget_micros: budget,
    };
    out.push(candidate(
        session,
        format!("cost:{}:{budget}", session.id()),
        NotificationRuleId::CostBudget,
        NotificationSeverity::Warning,
        delivery,
        "Session cost budget crossed",
        "Observed or projected session cost exceeds its budget.".into(),
        None,
        None,
        evidence,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ct_domain::{AgentKind, SessionId, SessionMetadata, TokenUsage, Turn};

    fn session(prompts: &[u32], window: u32) -> AgentSession {
        let turns = prompts
            .iter()
            .enumerate()
            .map(|(index, prompt)| Turn {
                number: TurnNumber::new(index as u32 + 1).unwrap(),
                timestamp: None,
                model: None,
                usage: TokenUsage {
                    input: Some(*prompt),
                    context_window: Some(window),
                    ..Default::default()
                },
                event_indices: Vec::new(),
                anchor_index: None,
            })
            .collect();
        AgentSession::new(
            SessionId::new("notice-session").unwrap(),
            AgentKind::Codex,
            SessionMetadata {
                context_window: Some(window),
                ..Default::default()
            },
            Vec::new(),
            turns,
            Vec::new(),
        )
    }

    #[test]
    fn pressure_notifies_only_on_upward_band_crossings() {
        let session = session(&[100, 760, 800, 910], 1_000);
        let prior = SessionNotificationCheckpoint::baseline(session.agent(), session.id().clone());
        let settings = NotificationSettings {
            enabled: true,
            ..NotificationSettings::default()
        };
        let result = NotificationEngine::evaluate(
            &session,
            &prior,
            &settings,
            NotificationInputs::default(),
        );
        let pressure = result
            .candidates
            .iter()
            .filter(|item| item.rule == NotificationRuleId::ContextPressure)
            .count();
        assert_eq!(pressure, 2);
        assert_eq!(result.checkpoint.pressure_band, PressureBand::Critical);
    }

    #[test]
    fn checkpoint_prevents_historical_spikes_from_replaying() {
        let session = session(&[10_000, 40_000], 100_000);
        let prior = NotificationEngine::baseline(&session);
        let settings = NotificationSettings {
            enabled: true,
            ..NotificationSettings::default()
        };
        let result = NotificationEngine::evaluate(
            &session,
            &prior,
            &settings,
            NotificationInputs::default(),
        );
        assert!(result.candidates.is_empty());
    }

    #[test]
    fn tool_failure_grouping_is_target_specific_without_persisting_the_target() {
        let first = tool_failure_key("shell", Some("C:\\private\\one.ps1"));
        let second = tool_failure_key("shell", Some("C:\\private\\two.ps1"));

        assert_ne!(first, second);
        assert!(first.starts_with("shell:"));
        assert!(!first.contains("private"));
        assert_eq!(tool_failure_key("shell", None), "shell");
    }
}
