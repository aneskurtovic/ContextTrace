//! Presentation. The only layer allowed to know about terminals.

use crate::format::{
    bar, bytes, confidence_tag, ellipsize, ellipsize_middle, pad, percent, rpad, thousands,
    token_count,
};
use ct_adapters::FileRawEventSource;
use ct_application::{ContextTrace, Diagnostics, ResidualPoint, ResolvedSession};
use ct_domain::model::event::EventKind;
use ct_domain::ports::RawEventSource;
use ct_domain::services::DerivedRatio;
use ct_domain::{AgentSession, FilteredView, SessionDescriptor};

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
        "{}  {}  {}  {}  PROJECT",
        pad("ID", 10),
        pad("AGENT", 12),
        pad("LAST ACTIVITY", 18),
        rpad("SIZE", 9)
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
        EventKind::Reasoning { redacted, .. } => {
            if *redacted {
                "reasoning (text redacted in log)".into()
            } else {
                "reasoning".into()
            }
        }
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

pub fn context(
    view: &FilteredView<'_>,
    estimator: &str,
    derived: Option<DerivedRatio>,
    json: bool,
) {
    if json {
        print_json(&view.composition_report());
        return;
    }

    let snapshot = view.snapshot();

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
    coverage(view);
    println!();

    let rows = view.by_category();
    if rows.is_empty() {
        nothing_matched(view);
        return;
    }
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

    // The narrative below describes the whole turn -- the unlogged remainder,
    // the fitted ratio, the calibration factor. Printed under a filter it would
    // read as commentary on the rows above, which it is not: those rows are a
    // slice, and the residual is deliberately not in them.
    if view.filter().is_active() {
        println!(
            "\n  These rows are part of turn {}, not all of it. Run without filters to\n  \
             see the full composition, the unlogged remainder and how it was measured.",
            snapshot.turn()
        );
        return;
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

    if let Some(ratio) = derived {
        println!(
            "\n  Ratio      {:.2} characters per token, measured from this session's own\n  \
             {:>10} usage across {} turn pairs (spread {:.1}x).",
            ratio.chars_per_token, "", ratio.pairs_used, ratio.dispersion
        );
        match ratio.unlogged_overhead {
            Some(overhead) => println!(
                "  Unlogged   ~{} tokens the agent never wrote down -- its system prompt\n  \
                 {:>10} and tool JSON schemas. Measured, not assumed.",
                thousands(overhead),
                ""
            ),
            // The measurement came out negative, which means reconstruction
            // accounted for more content than the prompt held. Reporting "0
            // hidden tokens" would turn a broken measurement into a confident
            // and wrong inventory.
            None => println!(
                "  Unlogged   not measurable here: reconstruction accounted for more content\n  \
                 {:>10} than the reported prompt held, so the hidden remainder cannot be\n  \
                 {:>10} separated from the over-count. Treat the rows as proportions.",
                "", ""
            ),
        }
        if ratio.dispersion > 2.0 {
            println!(
                "  The per-turn ratios varied widely, so this session mixes content that\n  \
                 tokenizes very differently. The ratio is a middle value, not a constant."
            );
        }
    }

    if let Some(scale) = snapshot.calibration_scale() {
        println!(
            "\n  Calibration: estimates scaled by {scale:.2} to meet the observed total of {}.",
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

pub fn largest(view: &FilteredView<'_>, derived: Option<DerivedRatio>, limit: usize, json: bool) {
    if json {
        print_json(&view.contributor_report(limit));
        return;
    }

    let snapshot = view.snapshot();
    let top = view.largest_contributors(limit);

    println!(
        "Largest context contributors at turn {} (total {})",
        snapshot.turn(),
        token_count(snapshot.total())
    );
    coverage(view);
    println!();

    if top.is_empty() {
        nothing_matched(view);
        return;
    }

    for item in &top {
        println!(
            "{}  {}  {}  {}",
            rpad(&thousands(item.tokens), 9),
            rpad(&percent(item.share), 6),
            pad(item.category.label(), 22),
            ellipsize_middle(&item.label, 62)
        );
        println!(
            "{}  from {} {}",
            " ".repeat(17),
            item.source,
            confidence_tag(item.confidence)
        );
    }

    // The residual may only be *called* the system prompt and tool schemas when
    // it was actually measurable. Where reconstruction over-counted, the derived
    // overhead is `None` and this row would be a caption invented for arithmetic
    // left over from a broken measurement -- which is precisely the overclaim
    // `ct context` suppresses, so it must be suppressed here too.
    if view.filter().is_active() {
        println!(
            "\nShares are of the turn's full {} tokens, so these rows deliberately do not\n\
             add up to 100%. {} of {} items matched.",
            thousands(snapshot.total().tokens()),
            view.matched_items(),
            view.total_items()
        );
        return;
    }

    if snapshot.residual_is_meaningful() && may_name_the_residual(derived) {
        println!(
            "\n{}  {}  unattributed (system prompt + tool schemas)",
            rpad(&thousands(snapshot.residual()), 9),
            rpad(
                &percent(snapshot.residual() as f32 / snapshot.total().tokens().max(1) as f32),
                6
            )
        );
    } else if let Some(ratio) = derived {
        if ratio.unlogged_overhead.is_none() {
            println!(
                "\nThe unlogged remainder is not measurable for this session -- reconstruction\n\
                 accounted for more content than the reported prompt held. These rows are\n\
                 proportions of the observed total, not a complete inventory."
            );
        }
    }
}

/// State the filter and how much of the turn survived it.
///
/// **The line that keeps a filtered view honest.** Four tool outputs shown alone
/// look like the whole context; saying they are 31% of it, against a total the
/// agent itself reported, is the difference between a finding and a distortion.
fn coverage(view: &FilteredView<'_>) {
    if !view.filter().is_active() {
        return;
    }
    println!("Filter     {}", view.filter());
    println!(
        "           {} of {} items, {} of {} tokens - {} of this turn{}",
        view.matched_items(),
        view.total_items(),
        thousands(view.matched_tokens()),
        thousands(view.total().tokens()),
        percent(view.share_of_total()),
        if view.residual_included() {
            ""
        } else {
            ", excluding the unattributed remainder"
        }
    );
}

/// Explain an empty result by saying what the turn actually contains.
///
/// A filter matching nothing is indistinguishable from a broken flag unless the
/// tool says which values would have worked. That matters more than usual here:
/// per-item sizes are `estimated` for both agents today, so `--confidence
/// observed` correctly matches nothing on every real session.
fn nothing_matched(view: &FilteredView<'_>) {
    println!("Nothing in this turn matches that filter.\n");

    println!("Categories present:");
    for (category, tokens) in view.available_categories() {
        println!(
            "  {}  {}",
            pad(&category.slug(), 24),
            rpad(&thousands(tokens), 9)
        );
    }

    println!("\nSources present:");
    for (kind, tokens) in view.available_sources() {
        println!(
            "  {}  {}",
            pad(kind.slug(), 24),
            rpad(&thousands(tokens), 9)
        );
    }

    let confidences: Vec<&str> = view
        .available_confidences()
        .into_iter()
        .map(|c| c.label())
        .collect();
    println!("\nConfidence levels present: {}", confidences.join(", "));
    if !confidences.contains(&"observed") {
        println!(
            "  No item is `observed`: per-item sizes are estimates for both agents, and\n  \
             only the turn total is a figure the agent itself reported."
        );
    }
}

pub fn residual(
    series: &[ResidualPoint],
    ratio: DerivedRatio,
    compaction_turns: &[u32],
    json: bool,
) {
    if json {
        print_json(series);
        return;
    }

    if series.is_empty() {
        println!("No turns with recorded usage in that range.");
        return;
    }

    println!(
        "Unlogged context per turn - {:.2} characters per token, measured from this\n\
         session across {} turn pairs\n",
        ratio.chars_per_token, ratio.pairs_used
    );
    println!(
        "{}  {}  {}  {}  SHARE",
        rpad("TURN", 6),
        rpad("PROMPT", 10),
        rpad("ACCOUNTED", 10),
        rpad("UNLOGGED", 10)
    );

    for point in series {
        let (unlogged, share) = match point.unlogged {
            Some(u) => (
                thousands(u),
                percent(u as f32 / point.prompt_tokens.max(1) as f32),
            ),
            // Over-counted: the remainder is unknown, and printing a zero here
            // would assert an inventory the reconstruction cannot support.
            None => ("-".to_string(), "over-counted".to_string()),
        };
        println!(
            "{}  {}  {}  {}  {}",
            rpad(&point.turn.to_string(), 6),
            rpad(&thousands(point.prompt_tokens), 10),
            rpad(&thousands(point.accounted), 10),
            rpad(&unlogged, 10),
            share
        );
    }

    let steps = ct_application::residual_steps(series);

    println!(
        "\n  This column drifts: one fitted ratio cannot describe a session that starts\n  \
         as prose and ends dominated by tool output, and whatever the ratio gets wrong\n  \
         lands here. Read the trend, not the turn-to-turn wiggle."
    );

    if steps.is_empty() {
        println!(
            "\nNo sustained change in the unlogged remainder."
        );
        return;
    }

    println!("\nSustained changes in unlogged context:");
    let mut any_unexplained = false;
    for step in &steps {
        let growth = step.growth();
        // A compaction rewrites the whole prompt, so a step next to one has an
        // obvious cause already in the log. Attributing it to an unrecorded
        // harness change would be inventing a second explanation for something
        // the session already accounts for.
        let near_compaction = compaction_turns
            .iter()
            .any(|t| t.abs_diff(step.turn) <= 5);
        if !near_compaction {
            any_unexplained = true;
        }
        println!(
            "  turn {}  {}{} tokens  ({} -> {}){}",
            step.turn,
            if growth > 0 { "+" } else { "-" },
            thousands(growth.unsigned_abs() as u32),
            thousands(step.from),
            thousands(step.to),
            if near_compaction {
                "   [a compaction occurred here, which explains it]"
            } else {
                ""
            }
        );
    }

    println!(
        "\n  These compare the median of the five turns either side, so they survive the\n  \
         drift above: a change that holds is one the harness made, while fit wobble\n  \
         reverts. Treat a step as evidence that something changed, not as its size."
    );
    if any_unexplained {
        println!(
            "\n  A rise means the prompt gained content the log does not record -- a tool\n  \
             registered, an MCP server connected, a skill loaded. A fall means the\n  \
             reconstruction began accounting for more of the prompt than before, which\n  \
             is either hidden content going away or the over-counting described in\n  \
             `ct context`. This view cannot tell those two apart."
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

    if diagnostics.multi_call_turns > 0 {
        println!(
            "Multi-call {} turn(s) were produced by more than one API call. Their prompt\n           \
             size is taken from the largest single call, because the log's top-level\n           \
             cache figures are the sum across calls and would overstate the prompt.",
            diagnostics.multi_call_turns
        );
    }

    if diagnostics.redacted_reasoning > 0 {
        println!(
            "Redacted   {} reasoning event(s) had their text stripped from the log. The\n           \
             reasoning still occupied context, so its size is derived from the leftover\n           \
             signature rather than measured.",
            diagnostics.redacted_reasoning
        );
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

/// Whether the leftover tokens may be *called* the system prompt and tool
/// schemas.
///
/// Only when the session's unlogged overhead was actually measurable. Where
/// reconstruction over-counted, the leftover is arithmetic from a broken
/// measurement, and captioning it would be exactly the overclaim this tool
/// exists to prevent. Where no ratio was derived at all -- Codex, whose items are
/// counted exactly -- the residual is trustworthy and may be named.
fn may_name_the_residual(derived: Option<DerivedRatio>) -> bool {
    derived.is_none_or(|r| r.unlogged_overhead.is_some())
}

fn print_json<T: serde::Serialize + ?Sized>(value: &T) {
    match serde_json::to_string_pretty(value) {
        Ok(text) => println!("{text}"),
        Err(e) => eprintln!("error: could not serialise output: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ratio(overhead: Option<u32>) -> DerivedRatio {
        DerivedRatio {
            chars_per_token: 2.4,
            pairs_used: 40,
            unlogged_overhead: overhead,
            dispersion: 1.4,
        }
    }

    #[test]
    fn an_unmeasurable_overhead_is_never_captioned_as_the_system_prompt() {
        // The regression this guards: `ct context` reported "not measurable"
        // while `ct largest` printed the same session's leftover tokens labelled
        // "system prompt + tool schemas". Two views, one session, contradictory
        // confidence.
        assert!(!may_name_the_residual(Some(ratio(None))));
    }

    #[test]
    fn a_measured_overhead_may_be_named() {
        assert!(may_name_the_residual(Some(ratio(Some(36_506)))));
    }

    #[test]
    fn an_agent_with_exact_counts_keeps_its_residual() {
        // Codex derives no ratio because tiktoken counts its items exactly, so
        // suppressing its residual would hide a figure that is trustworthy.
        assert!(may_name_the_residual(None));
    }
}
