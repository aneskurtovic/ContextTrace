//! Presentation. The only layer allowed to know about terminals.

use crate::format::{bar, bytes, confidence_tag, ellipsize, pad, percent, rpad, thousands, token_count};
use ct_adapters::FileRawEventSource;
use ct_application::{ContextTrace, Diagnostics, ResolvedSession};
use ct_domain::model::event::EventKind;
use ct_domain::ports::RawEventSource;
use ct_domain::{AgentSession, ContextSnapshot, SessionDescriptor};

pub fn roots(app: &ContextTrace) {
    println!("ContextTrace reads these local directories (read-only):\n");
    for (agent, paths) in app.roots() {
        println!("  {agent}");
        if paths.is_empty() {
            println!("    (none found)");
        }
        for path in paths {
            println!("    {path}");
        }
    }
    println!("\nNothing is written to them, and nothing leaves this machine.");
}

pub fn sessions(list: &[SessionDescriptor], json: bool) {
    if json {
        print_json(list);
        return;
    }

    if list.is_empty() {
        println!("No sessions found. Run `ct roots` to see which directories were searched.");
        return;
    }

    println!(
        "{}  {}  {}  {}  {}",
        pad("ID", 10),
        pad("AGENT", 12),
        pad("LAST ACTIVITY", 18),
        rpad("SIZE", 9),
        "PROJECT"
    );

    for d in list {
        let short_id: String = d.id.as_str().chars().take(8).collect();
        let when = d
            .last_activity
            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
            .unwrap_or_else(|| "-".into());
        println!(
            "{}  {}  {}  {}  {}",
            pad(&short_id, 10),
            pad(d.agent.label(), 12),
            pad(&when, 18),
            rpad(&bytes(d.size_bytes), 9),
            ellipsize(d.project.as_deref().unwrap_or("-"), 60)
        );
    }

    println!("\n{} session(s). Inspect one with: ct inspect <id>", list.len());
}

pub fn inspect(
    session: &AgentSession,
    resolved: &ResolvedSession,
    limit: usize,
    raw: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if json {
        print_json(session);
        return Ok(());
    }

    let meta = session.metadata();
    println!("Session   {}", session.id());
    println!("Agent     {}", session.agent());
    println!("File      {}", resolved.descriptor.path);
    if let Some(project) = &meta.project {
        println!("Project   {project}");
    }
    if let Some(model) = &meta.model {
        println!("Model     {model}");
    }
    if let (Some(branch), Some(commit)) = (&meta.git_branch, &meta.git_commit) {
        let short: String = commit.chars().take(8).collect();
        println!("Git       {branch} @ {short}");
    }
    println!("Turns     {}", session.turn_count());
    if let Some(peak) = session.peak_prompt_tokens() {
        println!("Peak      {} prompt tokens [observed]", thousands(peak));
    }
    println!("Fidelity  {}", percent(session.fidelity()));
    println!();

    let events = session.events();
    let shown = events.len().min(limit);
    println!("Timeline (showing {shown} of {} events)\n", events.len());

    let source = FileRawEventSource::for_session(&resolved.descriptor.path);

    for event in events.iter().take(limit) {
        let turn = event
            .turn
            .map(|t| format!("t{t}"))
            .unwrap_or_else(|| "-".into());
        let size = event
            .char_len()
            .map(|c| format!("{} ch", thousands(c)))
            .unwrap_or_default();

        println!(
            "{}  {}  {}  {}",
            rpad(&event.source.line_no.to_string(), 6),
            pad(&turn, 5),
            pad(&describe_kind(event), 46),
            size
        );

        if raw {
            match source.fetch_preview(event.source, 300) {
                Ok(text) => println!("        raw: {}", ellipsize(&text, 300)),
                Err(e) => println!("        raw: <unavailable: {e}>"),
            }
        }
    }

    if events.len() > limit {
        println!("\n... {} more events (use --limit)", events.len() - limit);
    }
    Ok(())
}

fn describe_kind(event: &ct_domain::Event) -> String {
    match &event.kind {
        EventKind::SessionStarted => "session started".into(),
        EventKind::TurnStarted => "turn started".into(),
        EventKind::TurnCompleted => "turn completed".into(),
        EventKind::Message { role, preview, .. } => {
            let role = format!("{role:?}").to_lowercase();
            if preview.is_empty() {
                format!("{role} message")
            } else {
                format!("{role}: {}", ellipsize(preview, 32))
            }
        }
        EventKind::Reasoning { .. } => "reasoning".into(),
        EventKind::ToolCall { tool, .. } => format!("tool call: {}", ellipsize(tool, 30)),
        EventKind::ToolResult { tool, is_error, .. } => {
            let name = tool.as_deref().unwrap_or("tool");
            if *is_error {
                format!("tool ERROR: {}", ellipsize(name, 28))
            } else {
                format!("tool output: {}", ellipsize(name, 28))
            }
        }
        EventKind::ContextInjection { label, .. } => {
            format!("injected: {}", ellipsize(label, 32))
        }
        EventKind::Compacted(facts) => match (facts.tokens_before, facts.tokens_after) {
            (Some(b), Some(a)) => format!(
                "COMPACTION {} -> {}",
                thousands(b),
                thousands(a)
            ),
            _ => "COMPACTION".into(),
        },
        EventKind::TokenReport(usage) => match usage.prompt_tokens() {
            Some(t) => format!("token report: {} prompt", thousands(t)),
            None => "token report".into(),
        },
        EventKind::SessionEvent { subtype } => format!("event: {}", ellipsize(subtype, 32)),
        // Made loud on purpose: an unknown type is the signal that the agent
        // changed its format and reconstruction may be incomplete.
        EventKind::Unrecognised => format!("UNRECOGNISED: {}", ellipsize(&event.raw_type, 28)),
    }
}

pub fn context(snapshot: &ContextSnapshot, estimator: &str, json: bool) {
    if json {
        print_json(snapshot);
        return;
    }

    println!(
        "Context at turn {} - {}",
        snapshot.turn(),
        token_count(snapshot.total())
    );
    if let Some(model) = snapshot.model() {
        println!("Model      {model}");
    }
    if let Some(window) = snapshot.context_window() {
        let used = snapshot
            .utilisation()
            .map(|u| format!(" ({} of window)", percent(u)))
            .unwrap_or_default();
        println!("Window     {}{used}", thousands(window));
    }
    println!("Estimator  {estimator}");

    if let Some(compaction) = snapshot.preceding_compaction() {
        let detail = match compaction.reduction() {
            Some(r) => format!("reclaimed {} tokens", thousands(r)),
            None => "size not reported".into(),
        };
        println!(
            "Compaction earlier in session ({}), {detail}",
            compaction.facts.trigger.as_deref().unwrap_or("unknown trigger")
        );
    }
    println!();

    let rows = snapshot.by_category();
    let widest = rows
        .iter()
        .map(|r| r.category.label().chars().count())
        .max()
        .unwrap_or(20);

    for row in &rows {
        println!(
            "  {}  {}  {}  {}  {}",
            pad(row.category.label(), widest),
            rpad(&thousands(row.tokens), 9),
            rpad(&percent(row.share), 6),
            bar(row.share, 20),
            confidence_tag(row.confidence)
        );
    }

    // Only call the residual "unlogged context" when it is big enough to
    // actually be that. Below the threshold it is arithmetic rounding, and
    // saying otherwise would be the exact kind of confident overclaim this tool
    // exists to prevent.
    if snapshot.residual_is_meaningful() {
        println!(
            "\n  The unattributed {} tokens are context the agent did not log -- in\n  \
             practice its system prompt and tool JSON schemas.",
            thousands(snapshot.residual())
        );
    }

    if let Some(scale) = snapshot.calibration_scale() {
        println!(
            "\n  Calibration: heuristic estimates scaled by {scale:.2} to meet the observed\n  \
             total of {}.",
            thousands(snapshot.total().tokens())
        );
        if scale < 0.95 && !snapshot.residual_is_meaningful() {
            println!(
                "  The estimator ran {:.0}% high, so the scaled figures consumed the whole\n  \
                 budget and no residual remains. That does NOT mean there is no hidden\n  \
                 context -- the system prompt and tool schemas are still in the total, and\n  \
                 their share has been absorbed into the categories above. Treat the\n  \
                 breakdown as proportions, not as an inventory.",
                (1.0 / scale - 1.0) * 100.0
            );
        }
    }

    println!(
        "\n{} context items. Largest contributors: ct largest <id> --turn {}",
        snapshot.items().len(),
        snapshot.turn()
    );
}

pub fn largest(snapshot: &ContextSnapshot, limit: usize, json: bool) {
    let top = snapshot.largest_contributors(limit);

    if json {
        print_json(&top);
        return;
    }

    println!(
        "Largest context contributors at turn {} (total {})\n",
        snapshot.turn(),
        token_count(snapshot.total())
    );

    if top.is_empty() {
        println!("No attributable context items at this turn.");
        return;
    }

    for item in &top {
        println!(
            "{}  {}  {}  {}",
            rpad(&thousands(item.tokens), 9),
            rpad(&percent(item.share), 6),
            pad(item.category.label(), 24),
            ellipsize(&item.label, 60)
        );
        println!(
            "{}  from {} {}",
            " ".repeat(17),
            item.source,
            confidence_tag(item.confidence)
        );
    }

    if snapshot.residual_is_meaningful() {
        println!(
            "\n{}  {}  {}",
            rpad(&thousands(snapshot.residual()), 9),
            rpad(
                &percent(snapshot.residual() as f32 / snapshot.total().tokens().max(1) as f32),
                6
            ),
            "unattributed (system prompt + tool schemas)"
        );
    }
}

pub fn doctor(
    diagnostics: &Diagnostics,
    session: &AgentSession,
    resolved: &ResolvedSession,
    json: bool,
) {
    if json {
        print_json(diagnostics);
        return;
    }

    println!("Doctor report for {}\n", session.id());
    println!("File       {}", resolved.descriptor.path);
    println!("Agent      {}", session.agent());
    println!("Parse      {}", diagnostics.headline());
    println!("Turns      {}", diagnostics.turns);

    if diagnostics.turns_without_usage > 0 {
        println!(
            "           {} turn(s) had no usage reported, so their context size is unknown",
            diagnostics.turns_without_usage
        );
    }

    if let (Some(peak), Some(turn)) = (diagnostics.peak_prompt_tokens, diagnostics.peak_turn) {
        println!("Peak       {} tokens at turn {turn}", thousands(peak));
    }

    println!("Compaction {} event(s)", diagnostics.compactions);
    if let Some(reduction) = diagnostics.compaction_reduction {
        println!("           {} tokens reclaimed [observed]", thousands(reduction));
    }

    if !diagnostics.unrecognised_types.is_empty() {
        println!("\nUnrecognised event types:");
        for (name, count) in &diagnostics.unrecognised_types {
            println!("  {}  {name}", rpad(&count.to_string(), 6));
        }
        println!(
            "\n  These are recorded but not interpreted. Context reconstruction for this\n  \
             session may be incomplete. Their raw lines remain visible via `ct inspect --raw`."
        );
    }

    if diagnostics.spikes.is_empty() {
        println!("\nNo context spikes above 20,000 tokens.");
    } else {
        println!("\nContext spikes:");
        for spike in &diagnostics.spikes {
            println!(
                "  turn {}  +{} tokens  ({} -> {})",
                spike.turn,
                thousands(spike.growth),
                thousands(spike.previous_tokens),
                thousands(spike.tokens)
            );
            for candidate in &spike.candidates {
                println!("      candidate: {}", ellipsize(candidate, 70));
            }
        }
        println!(
            "\n  Candidates are events recorded within the turn, ranked by size. The log\n  \
             shows they occurred; it does not prove which one caused the growth."
        );
    }
}

fn print_json<T: serde::Serialize + ?Sized>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("error: could not serialise output: {e}"),
    }
}
