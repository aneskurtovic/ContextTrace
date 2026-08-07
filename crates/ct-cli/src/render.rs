//! Presentation. The only layer allowed to know about terminals.

use crate::format::{
    bar, bytes, confidence_tag, ellipsize, ellipsize_middle, pad, percent, rpad, signed, thousands,
    token_count, wrap,
};
use ct_adapters::FileRawEventSource;
use ct_application::{
    AppError, Comparability, ContextTrace, Departure, Diagnostics, DriftReport, ExportRedaction,
    GrowthTimeline, ItemLifecycle, ResidualPoint, ResolvedSession, SecretScanReport, SessionDiff,
};
use ct_domain::model::event::EventKind;
use ct_domain::ports::{ExactRecount, PortError, RawEventSource};
use ct_domain::services::DerivedRatio;
use ct_domain::{
    AgentKind, AgentSession, CompactionDiff, CompactionItemDisposition, ContextSnapshot,
    Contributor, FilteredView, SessionDescriptor, TokenCount,
};

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

    println!(
        "\n{} session(s). Inspect one with: ct inspect <id>",
        list.len()
    );
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

/// Render a content-free structural accounting of Codex replacement histories.
pub fn compactions(report: &[CompactionDiff], json: bool) {
    if json {
        print_json(report);
        return;
    }
    if report.is_empty() {
        println!("No compactions recorded.");
        return;
    }

    for diff in report {
        match diff {
            CompactionDiff::Unavailable {
                source,
                turn,
                reason,
            } => {
                let turn = turn.map(|t| format!(" turn {t}")).unwrap_or_default();
                println!(
                    "Compaction at line {}{turn}: unavailable ({reason:?})",
                    source.line_no
                );
            }
            CompactionDiff::Available {
                source,
                turn,
                items,
            } => {
                let turn = turn.map(|t| format!(" turn {t}")).unwrap_or_default();
                let dropped = items
                    .iter()
                    .filter(|item| {
                        matches!(item.disposition, CompactionItemDisposition::Dropped { .. })
                    })
                    .count();
                println!(
                    "Compaction at line {}{turn}: {dropped} item(s) dropped [derived]",
                    source.line_no
                );
                println!(
                    "  STATE        POSITION                             TYPE                      ROLE       JSON BYTES  TEXT TOKENS"
                );
                for item in items {
                    let (state, position) = match item.disposition {
                        CompactionItemDisposition::Dropped { history_index } => {
                            ("dropped", format!("history #{history_index}"))
                        }
                        CompactionItemDisposition::Preserved {
                            history_index,
                            replacement_index,
                        } => (
                            "preserved",
                            format!("history #{history_index} -> replacement #{replacement_index}"),
                        ),
                        CompactionItemDisposition::AddedByReplacement { replacement_index } => {
                            ("replacement", format!("replacement #{replacement_index}"))
                        }
                    };
                    let role = item
                        .role
                        .as_ref()
                        .map(|role| format!("{role:?}").to_ascii_lowercase())
                        .unwrap_or_else(|| "-".into());
                    let tokens = item
                        .text_tokens
                        .map(|tokens| format!("{} [derived]", tokens.tokens()))
                        .unwrap_or_else(|| "opaque / structured".into());
                    println!(
                        "  {}  {}  {}  {}  {:>10}  {}",
                        pad(state, 11),
                        pad(&position, 35),
                        pad(&item.item_type, 24),
                        pad(&role, 9),
                        item.normalized_json_bytes,
                        tokens
                    );
                }
                println!("  JSON bytes are normalized compact item bytes [derived]; text tokens are measured only for wholly textual items.");
                println!(
                    "  \"history #N\" indexes the pre-compaction history; \"replacement #N\" indexes replacement_history — the two lists are numbered separately."
                );
            }
        }
    }
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
        EventKind::OversizedToolResult { image_count, .. } => {
            format!("oversized tool output: {image_count} inline image(s)")
        }
        EventKind::ContextInjection { label, .. } => {
            format!("injected: {}", ellipsize(label, 32))
        }
        EventKind::Compacted(facts) => match (facts.tokens_before, facts.tokens_after) {
            (Some(b), Some(a)) => format!("COMPACTION {} -> {}", thousands(b), thousands(a)),
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

/// What an `--exact` run actually managed to measure.
///
/// Printed rather than implied, because "exact" is a claim about the numbers
/// underneath it and a partial recount does not support the whole claim.
///
/// The over-count case is the one that would otherwise bite. The calibrator's
/// standing rule is that measurements exceeding the observed total have been
/// disproved by it, so it rescales them and relabels them `Calibrated`. Without
/// this notice, `--exact` would quietly print calibrated figures under a flag
/// whose whole purpose is to stop that happening.
fn exactness(recount: Option<ExactRecount>, snapshot: &ContextSnapshot) {
    let Some(report) = recount else { return };
    let estimated = report.opaque + report.unavailable;

    if report.counted == 0 {
        println!(
            "Exact      requested, but none of the {} items could be measured; all are\n\
             {:>10} still estimated from character counts.",
            report.total(),
            ""
        );
        return;
    }

    println!(
        "Exact      {} of {} items measured with the tokenizer, {estimated} left estimated\n\
         {:>10} ({} not tokenizable text, {} unreadable)",
        report.counted,
        report.total(),
        "",
        report.opaque,
        report.unavailable
    );

    if !snapshot.items().iter().any(|i| i.tokens.is_trustworthy()) {
        println!(
            "{:>10} The measured counts came to more than the {} tokens the agent says it\n\
             {:>10} sent, so reconstruction over-includes at this turn. Every figure below\n\
             {:>10} has been rescaled to fit and is calibrated, not exact.",
            "",
            thousands(snapshot.total().tokens()),
            "",
            ""
        );
    }
}

pub fn context(
    view: &FilteredView<'_>,
    estimator: &str,
    derived: Option<DerivedRatio>,
    recount: Option<ExactRecount>,
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
    exactness(recount, snapshot);

    if let Some(compaction) = snapshot.preceding_compaction() {
        let detail = match compaction.reduction() {
            Some(r) => format!("reclaimed {} tokens", thousands(r)),
            None => "size not reported".into(),
        };
        println!(
            "Compaction earlier in session ({}), {detail}",
            compaction
                .facts
                .trigger
                .as_deref()
                .unwrap_or("unknown trigger")
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

    // Neither the duplicate nor the low-entropy detector below can look at an
    // item with no content measurement -- that is an opt-in pass over raw
    // payloads (`ct context --no-content-analysis` skips it entirely, and
    // items whose logs expose no content never get one either). The view
    // counts these over the same matched set it ranks the two sections from,
    // so "none detected" and "nothing was measurable" never read the same.
    let unmeasured = view.unmeasured_items();

    let duplicates = view.duplicate_content();
    if duplicates.is_empty() {
        println!(
            "\n  Duplicates  none detected{}.",
            if view.filter().is_active() {
                " among the matching items"
            } else {
                ""
            }
        );
        if unmeasured > 0 {
            println!(
                "  {unmeasured} item(s) had no content measurement and were not checked for duplicates."
            );
        }
    } else {
        const SHOWN_GROUPS: usize = 10;
        let copies = duplicates
            .iter()
            .map(|group| group.items.len())
            .sum::<usize>();
        let total = duplicates
            .iter()
            .fold(0u32, |sum, group| sum.saturating_add(group.total_tokens));
        let repeated = duplicates
            .iter()
            .fold(0u32, |sum, group| sum.saturating_add(group.repeated_tokens));
        println!(
            "\nExact duplicate content - {} groups, {copies} copies, {} tokens total,\n\
             {} repeated\n",
            duplicates.len(),
            thousands(total),
            thousands(repeated)
        );
        for group in duplicates.iter().take(SHOWN_GROUPS) {
            println!(
                "  {} copies  {} tokens total  {} repeated  {} {}",
                group.items.len(),
                thousands(group.total_tokens),
                thousands(group.repeated_tokens),
                percent(group.share),
                confidence_tag(group.confidence)
            );
            for item in &group.items {
                println!(
                    "      {}  {}  {}",
                    rpad(&thousands(item.tokens), 9),
                    item.id,
                    ellipsize_middle(&item.label, 62)
                );
            }
        }
        if duplicates.len() > SHOWN_GROUPS {
            let hidden = &duplicates[SHOWN_GROUPS..];
            let hidden_repeated = hidden
                .iter()
                .fold(0u32, |sum, group| sum.saturating_add(group.repeated_tokens));
            println!(
                "  ... {} more groups with {} repeated tokens (all groups are in --json)",
                hidden.len(),
                thousands(hidden_repeated)
            );
        }
        println!(
            "\n  Total is the footprint of every copy; repeated is the avoidable cost after\n  \
             keeping the first. Matching is byte-exact over model-visible content."
        );
        if unmeasured > 0 {
            println!(
                "  {unmeasured} more item(s) had no content measurement and were not checked."
            );
        }
    }

    let low_entropy = view.low_entropy_content();
    if low_entropy.is_empty() {
        println!(
            "\n  Low entropy  none detected{}.",
            if view.filter().is_active() {
                " among the matching items"
            } else {
                ""
            }
        );
        if unmeasured > 0 {
            println!(
                "  {unmeasured} item(s) had no content measurement and were not checked for low-information content."
            );
        }
    } else {
        const SHOWN_ITEMS: usize = 10;
        let total_score = low_entropy.iter().fold(0u32, |sum, item| {
            sum.saturating_add(item.waste_score_tokens)
        });
        println!(
            "\nLow-information blocks - {} large items, {} combined waste score\n",
            low_entropy.len(),
            thousands(total_score)
        );
        println!(
            "  {}  {}  {}  ITEM",
            rpad("SCORE", 10),
            rpad("TOKENS", 10),
            rpad("RATIO", 8)
        );
        for item in low_entropy.iter().take(SHOWN_ITEMS) {
            println!(
                "  {}  {}  {}  {}  {} {}",
                rpad(&thousands(item.waste_score_tokens), 10),
                rpad(&thousands(item.tokens), 10),
                rpad(&percent(item.compression_ratio), 8),
                item.id,
                ellipsize_middle(&item.label, 48),
                confidence_tag(item.confidence)
            );
        }
        if low_entropy.len() > SHOWN_ITEMS {
            println!(
                "  ... {} more item(s) (all findings are in --json)",
                low_entropy.len() - SHOWN_ITEMS
            );
        }
        println!(
            "\n  Ratio is DEFLATE bytes / original bytes. Score is tokens x (1 - ratio):\n  \
             a ranking heuristic for repetition, not a claim that those tokens are removable.\n  \
             Only visible payloads of at least 4 KiB with a ratio at or below 75% qualify."
        );
        if unmeasured > 0 {
            println!(
                "  {unmeasured} more item(s) had no content measurement and were not checked."
            );
        }
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
        // With nothing left estimated, the usual "plus whatever the estimates
        // missed" clause would be false -- and the remainder is still large, so
        // it has to be attributed to something rather than left implied. What is
        // left is real: an exact count measures an item's model-visible text,
        // not the framing the request wraps around it.
        let fully_measured =
            recount.is_some_and(|r| r.counted > 0 && r.opaque + r.unavailable == 0);
        if fully_measured {
            println!(
                "\n  The unattributed {} tokens are not estimation error -- every item at\n  \
                 this turn was counted exactly. What remains is the agent's tool JSON\n  \
                 schemas, plus the request framing around each item: field names, role\n  \
                 markers, block structure. `--exact` measures the text the model reads,\n  \
                 deliberately not the JSON it arrives in.",
                thousands(snapshot.residual())
            );
        } else if system_prompt_is_itemised(snapshot) {
            println!(
                "\n  The unattributed {} tokens are context the agent did not log -- in\n  \
                 practice its tool JSON schemas, plus whatever the per-item estimates\n  \
                 missed. This agent logs its system prompt, so that part is a row above\n  \
                 rather than part of this remainder.",
                thousands(snapshot.residual())
            );
        } else {
            println!(
                "\n  The unattributed {} tokens are context the agent did not log -- in\n  \
                 practice its system prompt and tool JSON schemas.",
                thousands(snapshot.residual())
            );
        }
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
                 context -- {} still in the total, and their\n  \
                 share has been absorbed into the categories above. Treat the breakdown\n  \
                 as proportions, not as an inventory.",
                (1.0 / scale - 1.0) * 100.0,
                if system_prompt_is_itemised(snapshot) {
                    "the tool schemas are"
                } else {
                    "the system prompt and tool schemas are"
                }
            );
        }
    }

    println!(
        "\n{} context items. Largest contributors: ct largest <id> --turn {}",
        snapshot.items().len(),
        snapshot.turn()
    );
}

pub fn largest(
    view: &FilteredView<'_>,
    derived: Option<DerivedRatio>,
    recount: Option<ExactRecount>,
    limit: usize,
    json: bool,
) {
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
    exactness(recount, snapshot);
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
        // The id is here because it is the argument to `ct trace`. Without it
        // the natural next step -- "how long has that 14k-token file been
        // sitting there?" -- has nothing to name the item with but a label that
        // is often a long path and is not unique.
        println!(
            "{}  {}  from {} {}",
            " ".repeat(17),
            item.id,
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
            "\n{}  {}  unattributed ({})",
            rpad(&thousands(snapshot.residual()), 9),
            rpad(
                &percent(snapshot.residual() as f32 / snapshot.total().tokens().max(1) as f32),
                6
            ),
            if system_prompt_is_itemised(snapshot) {
                "tool schemas + estimation error"
            } else {
                "system prompt + tool schemas"
            }
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

/// One item's size, taken from a single calibrated turn.
pub struct ItemSize {
    pub turn: u32,
    pub contributor: Contributor,
    pub turn_total: TokenCount,
}

/// One item's history: where it entered, how long it stayed, what removed it.
///
/// # Why there is one size and not a series
///
/// An item's text does not change while it sits in context -- the same log line
/// is replayed into every prompt that holds it. What *does* move between turns
/// is the calibration scale, so a per-turn size column would show the item
/// growing and shrinking when nothing about it changed. One figure, from one
/// turn, named as being from that turn.
///
/// Naming the turn is not decoration. This sizes at the last turn holding the
/// item, while `ct largest` defaults to the session's peak turn, so the same
/// item legitimately reads as 12.3% of 73,138 there and 2.6% of 339,687 here.
/// The token count is the same; only the denominator moved, and the line says
/// which one it used.
pub fn trace(life: &ItemLifecycle, size: Option<&ItemSize>, agent: AgentKind, json: bool) {
    if json {
        print_json(&TraceReport {
            life,
            size: size.map(|s| SizeReport {
                turn: s.turn,
                turn_total: s.turn_total.tokens(),
                item: &s.contributor,
            }),
        });
        return;
    }

    println!("Item      {}", life.id);
    println!("          {}", ellipsize_middle(&life.label, 68));
    println!("Category  {}, from {}", life.category.label(), life.source);

    match life.first_present() {
        Some(turn) => println!("\nEntered   turn {turn}"),
        None => {
            println!("\nThis item never appears in a reconstructed turn.");
            return;
        }
    }

    let spans: Vec<String> = life
        .runs
        .iter()
        .map(|r| {
            if r.from == r.to {
                r.from.to_string()
            } else {
                format!("{}-{}", r.from, r.to)
            }
        })
        .collect();
    println!(
        "Present   turn{} {}  ({} of {} turns scanned)",
        if life.turns_present() == 1 { "" } else { "s" },
        spans.join(", "),
        life.turns_present(),
        life.scanned_turns
    );

    match size {
        Some(size) => println!(
            "Size      {} tokens at turn {} - {} of that turn's {} {}",
            thousands(size.contributor.tokens),
            size.turn,
            percent(size.contributor.share),
            thousands(size.turn_total.tokens()),
            confidence_tag(size.contributor.confidence)
        ),
        // Never just omit the row. A silently missing figure reads as "this
        // item has no size", when what happened is that the turn could not be
        // calibrated -- which is a fact about the turn, not about the item.
        None => println!(
            "Size      not available: turn {} could not be calibrated, so there is no\n\
             {:10}total to state a share of",
            life.last_present().unwrap_or_default(),
            ""
        ),
    }

    match (&life.departure, life.still_present) {
        (_, true) => println!(
            "Status    still in context at turn {}, the last turn on this thread",
            life.last_scanned_turn.unwrap_or_default()
        ),
        (Some(Departure::Compaction { turn, reclaimed }), _) => println!(
            "Left      after turn {} - the compaction{} removed it{}",
            life.last_present().unwrap_or_default(),
            turn.map(|t| format!(" at turn {t}")).unwrap_or_default(),
            reclaimed
                .map(|r| format!(", reclaiming {} tokens", thousands(r)))
                .unwrap_or_default(),
        ),
        (Some(Departure::BranchDiverged { turn }), _) => println!(
            "Left      after turn {} - not evicted; turn {turn} is on another branch",
            life.last_present().unwrap_or_default()
        ),
        (Some(Departure::Unexplained { turn }), _) => println!(
            "Left      after turn {} - gone by turn {turn}, cause not established",
            life.last_present().unwrap_or_default()
        ),
        (None, false) => println!(
            "Left      after turn {} - the next turn could not be reconstructed, so\n\
             {:10}nothing can be said about why",
            life.last_present().unwrap_or_default(),
            ""
        ),
    }

    trace_notes(life, size, agent);
}

/// The caveats that keep the four lines above from being read as more than they
/// are. Printed only when they apply.
fn trace_notes(life: &ItemLifecycle, size: Option<&ItemSize>, agent: AgentKind) {
    if let Some(Departure::BranchDiverged { turn }) = life.departure {
        println!(
            "\n  No compaction removed this. Claude Code's log is a DAG, and turn {turn}\n  \
             descends from a different branch -- the conversation was rewound or a\n  \
             message edited, so the later turns continue from a history this item was\n  \
             never part of. It was not evicted from a prompt; it was never in theirs."
        );
    }

    if let Some(Departure::Unexplained { turn }) = life.departure {
        if agent == AgentKind::Codex {
            println!(
                "\n  This should not be possible. Codex reconstruction replays the API item\n  \
                 list forward and only clears it at a compaction, so an item vanishing at\n  \
                 turn {turn} without one is a defect in ContextTrace rather than something\n  \
                 that happened in the session. Please report it."
            );
        }
    }

    if !life.unknown_turns.is_empty() {
        let listed: Vec<String> = life
            .unknown_turns
            .iter()
            .take(8)
            .map(|t| t.to_string())
            .collect();
        println!(
            "\n  Turn(s) {}{} could not be reconstructed. Presence there is unknown, not\n  \
             absent, so the runs above stop at them rather than reading across them.",
            listed.join(", "),
            if life.unknown_turns.len() > listed.len() {
                format!(" and {} more", life.unknown_turns.len() - listed.len())
            } else {
                String::new()
            }
        );
    }

    if life.other_thread_turns > 0 {
        println!(
            "\n  {} turn(s) belong to the other thread and were excluded. A subagent runs\n  \
             against its own context window, so its turns say nothing about whether a\n  \
             main-thread item was present -- counting them would make every long-lived\n  \
             item appear to flicker in and out.",
            life.other_thread_turns
        );
    }

    if life.first_seen_disagrees() {
        println!(
            "\n  The log records this item's line as written during turn {}, but the first\n  \
             prompt observed to contain it is turn {}. Both are true: one is when the\n  \
             line appeared, the other is when it entered a request. This view reports\n  \
             the second.",
            life.recorded_first_seen.unwrap_or_default(),
            life.first_present().unwrap_or_default()
        );
    }

    if size.is_some() {
        println!(
            "\n  The size is measured once, at that turn. An item's text does not change\n  \
             while it sits in context; only the calibration scale moves, so a per-turn\n  \
             size column would show movement the item does not have."
        );
    }
}

/// Print the items a reference matched, so the user can pick one.
pub fn trace_candidates(candidates: &[ct_application::lifecycle::Candidate], json: bool) {
    if json {
        print_json(candidates);
        return;
    }

    println!("That reference matches {} items:\n", candidates.len());
    println!("{}  {}  LABEL", pad("ID", 16), pad("TURNS", 16));
    for candidate in candidates.iter().take(20) {
        let span = match (candidate.first_present, candidate.last_present) {
            (Some(a), Some(b)) if a == b => format!("{a}"),
            (Some(a), Some(b)) => format!("{a}-{b} ({})", candidate.turns_present),
            _ => "-".into(),
        };
        println!(
            "{}  {}  {}",
            pad(candidate.id.as_str(), 16),
            pad(&span, 16),
            ellipsize_middle(&candidate.label, 60)
        );
    }
    if candidates.len() > 20 {
        println!("... and {} more", candidates.len() - 20);
    }
    println!();
}

#[derive(serde::Serialize)]
struct TraceReport<'a> {
    #[serde(flatten)]
    life: &'a ItemLifecycle,
    size: Option<SizeReport<'a>>,
}

#[derive(serde::Serialize)]
struct SizeReport<'a> {
    turn: u32,
    turn_total: u32,
    item: &'a Contributor,
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
        println!("\nNo sustained change in the unlogged remainder.");
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
        let near_compaction = compaction_turns.iter().any(|t| t.abs_diff(step.turn) <= 5);
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

/// Format drift across a swept corpus.
///
/// The histogram is the deliverable, so it prints on the clean path too. A
/// sweep that finds nothing is a result — it is the evidence that this build
/// still understands both agents — and printing nothing would make a passing
/// CI run indistinguishable from a broken one.
pub fn drift(report: &DriftReport, json: bool) {
    if json {
        print_json(report);
        return;
    }

    println!("Format drift sweep\n");
    println!("Sessions   {} scanned", report.sessions_scanned);
    for (agent, count) in &report.scanned_by_agent {
        println!("           {count} {agent}");
    }
    println!(
        "Events     {} parsed",
        thousands(report.total_events as u64)
    );

    if report.sessions_scanned == 0 {
        if report.matched_nothing() {
            println!(
                "\nNo session lives under that path, so nothing was checked. Run `ct roots`\n\
                 to see the directories ContextTrace reads. This exits non-zero on purpose:\n\
                 a swept corpus of nothing must not read as a format that was verified."
            );
        } else {
            println!(
                "\nNo sessions found at all. Run `ct roots` to see where they are looked for."
            );
        }
        return;
    }

    if report.is_clean() {
        println!(
            "Recognised every event type in every session.\n\n\
             That is the claim worth re-running: both agents evolve their log formats,\n\
             and a type this build has not learned degrades every other command quietly."
        );
        return;
    }

    if !report.types.is_empty() {
        println!(
            "\nUnrecognised event types ({:.2}% fidelity)\n",
            report.fidelity() * 100.0
        );
        println!(
            "{}  {}  {}  TYPE",
            pad("AGENT", 12),
            rpad("EVENTS", 8),
            rpad("SESSIONS", 9)
        );
        for kind in &report.types {
            println!(
                "{}  {}  {}  {}",
                pad(&kind.agent.to_string(), 12),
                rpad(&thousands(kind.events), 8),
                rpad(&format!("{}/{}", kind.sessions, report.sessions_scanned), 9),
                kind.raw_type
            );
            println!("{}  ct inspect {} --raw", " ".repeat(33), kind.example);
        }
        println!(
            "\nA type in one session of many is a one-off or an aborted experiment. The\n\
             same type in most of them is a format change that has already shipped."
        );
    }

    if !report.unreadable.is_empty() {
        println!("\nUnreadable sessions\n");
        for session in &report.unreadable {
            println!(
                "  {}  {}",
                pad(&session.agent.to_string(), 12),
                session.path
            );
            println!("                {}", session.error);
        }
        println!(
            "\nThese are files that could not be parsed at all, which is a different\n\
             failure from an event type we have not learned -- more often a truncated\n\
             write or a permissions problem than a format change."
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
        // "no usable size", not "no usage record": an all-zero usage object is
        // counted here too, and it is a record that exists. See
        // `TokenUsage::prompt_tokens`.
        println!(
            "           {} turn(s) reported no usable size (absent or all-zero usage), \
             so their context size is unknown",
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
        println!(
            "           {} tokens reclaimed [observed]",
            thousands(reduction)
        );
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

/// Whether the leftover tokens may be *called* anything at all.
///
/// Only when the session's unlogged overhead was actually measurable. Where
/// reconstruction over-counted, the leftover is arithmetic from a broken
/// measurement, and captioning it would be exactly the overclaim this tool
/// exists to prevent. Where no ratio was derived at all -- Codex, for which no
/// per-session ratio is fitted -- there is no over-count measurement to fail, so
/// the remainder stands as what it is: the part of the observed total nothing
/// visible accounts for.
fn may_name_the_residual(derived: Option<DerivedRatio>) -> bool {
    derived.is_none_or(|r| r.unlogged_overhead.is_some())
}

/// Whether this agent logged its own system prompt as a visible item.
///
/// Codex records `base_instructions`, so for its sessions the system prompt is a
/// row in the breakdown -- and captioning the remainder "system prompt + tool
/// schemas" would name something the same screen has already listed, while
/// hiding that what is actually left is the tool schemas and the estimator's
/// error. Claude Code logs no system prompt, so there the caption is right.
fn system_prompt_is_itemised(snapshot: &ct_domain::ContextSnapshot) -> bool {
    snapshot
        .items()
        .iter()
        .any(|i| i.source == ct_domain::ContextSource::AgentSystemPrompt)
}

/// Write a session to stdout as NDJSON, one record per line.
///
/// Locked and buffered for the whole stream rather than going through
/// `println!`, which takes the lock and flushes per call: the largest local
/// session emits a few hundred thousand records, and paying that per line turns
/// a seconds-long export into a minutes-long one.
///
/// A broken pipe ends the export quietly. `ct export <id> | head` is the first
/// thing anyone tries, and it must not print an error for working exactly as
/// asked.
/// Prompt size across the session, as a sparkline.
///
/// Every figure here is the agent's own usage record. Nothing is reconstructed,
/// estimated or calibrated, which is why this view carries no confidence tags:
/// there is no guess in it to label.
pub fn growth(timeline: &GrowthTimeline, session: &AgentSession, width: usize, json: bool) {
    if json {
        #[derive(serde::Serialize)]
        struct Report<'a> {
            #[serde(flatten)]
            timeline: &'a GrowthTimeline,
            measured_turns: usize,
            gap_turns: usize,
            peak_turn: Option<u32>,
            peak_tokens: Option<u32>,
            largest_jumps: Vec<ct_application::Jump>,
        }
        let peak = timeline.peak();
        print_json(&Report {
            timeline,
            measured_turns: timeline.measured(),
            gap_turns: timeline.gaps(),
            peak_turn: peak.map(|(turn, _)| turn),
            peak_tokens: peak.map(|(_, tokens)| tokens),
            largest_jumps: timeline.largest_jumps(5),
        });
        return;
    }

    println!(
        "Context growth  {}  {}",
        session.id(),
        session.agent().label()
    );

    if timeline.points.is_empty() {
        println!("\n  No turns in that range.");
        return;
    }

    let gaps = timeline.gaps();
    println!(
        "  Turns      {}{}",
        thousands(timeline.points.len() as u64),
        match gaps {
            0 => String::new(),
            // Named rather than quietly excluded: these turns are on the chart
            // as blanks, and a reader counting bars would otherwise come up
            // short with no explanation.
            n => format!(", {n} of which the agent recorded no size for and are drawn as gaps"),
        }
    );

    match timeline.peak() {
        Some((turn, tokens)) => {
            let share = timeline
                .peak_utilisation()
                .map(|u| {
                    format!(
                        " ({} of a {} window)",
                        percent(u),
                        thousands(timeline.context_window.unwrap_or(0))
                    )
                })
                .unwrap_or_default();
            println!(
                "  Peak       {} tokens at turn {turn}{share}",
                thousands(tokens)
            );
        }
        None => println!("  Peak       not known -- no turn in this range recorded its size"),
    }

    sparkline(timeline, width);
    jumps_table(timeline);
}

/// The chart itself, plus the two lines that say what it means.
fn sparkline(timeline: &GrowthTimeline, width: usize) {
    const LEVELS: [char; 8] = [
        '\u{2581}', '\u{2582}', '\u{2583}', '\u{2584}', '\u{2585}', '\u{2586}', '\u{2587}',
        '\u{2588}',
    ];
    /// A turn whose size the agent never recorded. Deliberately not the shortest
    /// bar: the shortest bar is a small prompt, and this is not a prompt size.
    const GAP: char = '\u{00b7}';

    let buckets = timeline.buckets(width);
    let Some((_, peak)) = timeline.peak() else {
        // Say that the chart is missing and why. Returning silently leaves a
        // header with no chart under it, which reads as "flat" or as a display
        // bug -- and "nothing was measured" is a different statement from
        // either. Bars are drawn against the peak; without one there is no
        // scale to draw against.
        println!("\n  No chart: the bars are scaled against the session's peak, and no");
        println!("  turn in this range recorded a size, so there is no scale to draw on.");
        return;
    };

    let bars: String = buckets
        .iter()
        .map(|bucket| match bucket.peak {
            // Scaled against a zero baseline, so bar height is proportional to
            // tokens. Scaling from the minimum instead would turn a session
            // that grew 5% into one that appears to have grown tenfold.
            Some(tokens) => {
                let level = ((tokens as f32 / peak.max(1) as f32) * LEVELS.len() as f32).ceil();
                LEVELS[(level as usize).clamp(1, LEVELS.len()) - 1]
            }
            None => GAP,
        })
        .collect();

    let marks: String = buckets
        .iter()
        .map(|b| if b.compactions > 0 { 'c' } else { ' ' })
        .collect();

    println!("\n  {bars}");
    if timeline.compactions() > 0 {
        println!("  {}", marks.trim_end());
    }

    let per_column = timeline.turns_per_column(width);
    let first = timeline.points[0].turn;
    let last = timeline.points[timeline.points.len() - 1].turn;
    let axis = format!("turn {first}");
    let end = format!("turn {last}");
    println!(
        "  {}{}",
        pad(
            &axis,
            buckets.len().saturating_sub(end.chars().count()).max(1)
        ),
        end
    );

    println!();
    if per_column == 1 {
        println!("  One column per turn. Bar height is the prompt size against a zero baseline.");
    } else {
        // Stated because it changes what the chart can be read to mean: a fall
        // inside a column is invisible, and the compaction marks are the only
        // reason the biggest of those falls is not lost with it.
        println!(
            "  Each column is {per_column} turns, drawn at the largest prompt among them\n  \
             against a zero baseline. A fall within a column does not show."
        );
    }
    match timeline.compactions() {
        0 => {}
        n => println!("  'c' marks a column containing a compaction; there are {n}."),
    }
    if timeline.unplaced_compactions > 0 {
        println!(
            "  {} further compaction(s) happened but name no turn, so they are not on\n  \
             the chart. A fall with no 'c' under it may be one of them.",
            timeline.unplaced_compactions
        );
    }
}

fn jumps_table(timeline: &GrowthTimeline) {
    let jumps = timeline.largest_jumps(5);
    if jumps.is_empty() {
        return;
    }

    println!("\nLargest changes");
    for jump in &jumps {
        // A skipped turn means the change happened somewhere across a stretch
        // the agent did not record, so naming this turn as the cause would
        // assert more than the log supports.
        let across = match jump.skipped {
            0 => String::new(),
            n => format!("  (across {} unrecorded turn(s))", n),
        };
        println!(
            "  turn {}  {}  {} -> {}{across}",
            pad(&jump.turn.to_string(), 6),
            rpad(&signed(jump.growth()), 10),
            thousands(jump.from),
            thousands(jump.to),
        );
    }
}

/// Two turns side by side.
///
/// The layout follows the honesty gradient rather than the interest gradient:
/// the figures needing no caveat come first (prompt totals, item and call
/// counts), then the token composition, whose deltas are only as good as the
/// instruments behind them. A reader who stops after the header has still read
/// something true.
pub fn diff(diff: &SessionDiff, json: bool) {
    if json {
        #[derive(serde::Serialize)]
        struct Report<'a> {
            #[serde(flatten)]
            diff: &'a SessionDiff,
            /// Derived from the two totals. Emitted rather than left to the
            /// consumer because getting the direction wrong is silent.
            prompt_delta: i64,
            totals_are_observed: bool,
        }
        print_json(&Report {
            diff,
            prompt_delta: diff.prompt_delta(),
            totals_are_observed: diff.totals_are_observed(),
        });
        return;
    }

    println!(
        "Diff  {} -> {}",
        side_label(&diff.left),
        side_label(&diff.right)
    );
    println!();
    diff_side("LEFT ", &diff.left);
    diff_side("RIGHT", &diff.right);

    println!();
    if diff.totals_are_observed() {
        println!(
            "  Prompt     {} tokens, both sides read from the agents' own usage records",
            signed(diff.prompt_delta())
        );
    } else {
        println!(
            "  Prompt     {} tokens -- at least one side had no usage record and was \
             summed from\n             our own estimates, so this figure is not a measurement",
            signed(diff.prompt_delta())
        );
    }
    instrument_note(diff);

    composition_table(diff);
    tool_table(diff);
}

fn side_label(side: &ct_application::SideSummary) -> String {
    format!("{} @ turn {}", short_id(&side.session_id), side.turn)
}

/// Sessions are named by a prefix everywhere else in this tool, and a diff
/// header with two full UUIDs in it wraps on any normal terminal.
fn short_id(id: &str) -> &str {
    id.split_once('-').map_or(id, |(head, _)| head)
}

fn diff_side(tag: &str, side: &ct_application::SideSummary) {
    println!(
        "  {tag}  {}  {}  {}  {}  {} items",
        pad(short_id(&side.session_id), 10),
        pad(side.agent.label(), 11),
        pad(&format!("turn {}", side.turn), 9),
        rpad(&token_count(side.total), 20),
        thousands(side.items as u64),
    );
}

/// State how the two sides were measured, and what that permits.
fn instrument_note(diff: &SessionDiff) {
    match &diff.comparability {
        Comparability::Identical { estimator } => {
            let why = if diff.same_session() {
                "one session, so one instrument sized both turns"
            } else {
                "the same instrument sized both sides"
            };
            println!(
                "  Instrument {estimator} -- {why}.\n             \
                 Every delta below is content."
            );
        }
        Comparability::Skewed { left, right, skew } => {
            println!(
                "  Instrument {left} vs {right} -- {} apart.\n             \
                 Claude Code item sizes come from a ratio fitted to each session's own\n             \
                 usage, so these two sides are reported on differently graduated scales.\n             \
                 Each row below states how much of its delta that alone could explain.",
                percent(*skew)
            );
        }
        Comparability::Incomparable {
            left,
            right,
            reason,
        } => {
            println!(
                "  Instrument {left} vs {right}.\n             {}\n             \
                 The counts above and below are unaffected and are the comparison.",
                wrap(&format!("No token delta is reported: {reason}."), 66, 13)
            );
        }
    }
}

fn composition_table(diff: &SessionDiff) {
    // A category absent from both turns is not a finding, and there are
    // fourteen of them.
    let rows: Vec<&ct_application::CategoryDelta> = diff
        .categories
        .iter()
        .filter(|c| c.left > 0 || c.right > 0)
        .collect();
    if rows.is_empty() {
        return;
    }
    let comparable = diff.comparability.tokens_are_comparable();
    // A verdict column earns its width only where a delta can be both real and
    // explainable by the measurement. Under one instrument it never can be.
    let bounded = matches!(diff.comparability, Comparability::Skewed { .. });

    println!("\nComposition");
    let widest = rows
        .iter()
        .map(|c| c.category.label().chars().count())
        .max()
        .unwrap_or(20);
    println!(
        "  {}  {}  {}{}",
        pad("CATEGORY", widest),
        rpad("LEFT", 10),
        rpad("RIGHT", 10),
        if comparable {
            rpad("DELTA", 12)
        } else {
            String::new()
        }
    );

    for row in &rows {
        let delta = if comparable {
            let verdict = match (bounded, row.delta == 0, row.is_meaningful()) {
                (_, true, _) => String::new(),
                (false, _, _) => String::new(),
                (true, _, true) => "  changed".into(),
                (true, _, false) => format!(
                    "  within +-{}",
                    thousands(row.instrument_bound.unwrap_or(0))
                ),
            };
            format!("{}{verdict}", rpad(&signed(row.delta), 12))
        } else {
            String::new()
        };
        println!(
            "  {}  {}  {}{}",
            pad(row.category.label(), widest),
            rpad(&thousands(row.left), 10),
            rpad(&thousands(row.right), 10),
            delta
        );
    }

    // Item counts are the composition axis that survives every instrument
    // question, so where they moved it is worth saying outright.
    let moved: Vec<&ct_application::CategoryDelta> = diff
        .categories
        .iter()
        .filter(|c| c.item_delta() != 0)
        .collect();
    if !moved.is_empty() {
        let summary: Vec<String> = moved
            .iter()
            .take(4)
            .map(|c| {
                format!(
                    "{} {}",
                    signed(c.item_delta()),
                    c.category.label().to_lowercase()
                )
            })
            .collect();
        println!("\n  Item counts  {}", summary.join(", "));
    }
}

fn tool_table(diff: &SessionDiff) {
    if diff.tools.is_empty() {
        println!("\nTool usage   neither turn had a tool result in context");
        return;
    }

    const LIMIT: usize = 12;
    let bounded = matches!(diff.comparability, Comparability::Skewed { .. });
    println!("\nTool usage");
    let widest = diff
        .tools
        .iter()
        .take(LIMIT)
        .map(|t| t.tool.chars().count().min(24))
        .max()
        .unwrap_or(12)
        .max(4);
    println!(
        "  {}  {}  {}  {}  {}  {}",
        pad("TOOL", widest),
        rpad("CALLS", 6),
        rpad("", 6),
        rpad("DELTA", 7),
        rpad("LEFT TOK", 10),
        rpad("RIGHT TOK", 10),
    );

    for tool in diff.tools.iter().take(LIMIT) {
        // Only where a bound exists to clear. Under one instrument every
        // non-zero delta clears it, and a column reading "changed" on every row
        // says nothing.
        let flag = if bounded && tool.tokens_are_meaningful() {
            "  changed"
        } else {
            ""
        };
        println!(
            "  {}  {}  {}  {}  {}  {}{flag}",
            pad(&ellipsize(&tool.tool, 24), widest),
            rpad(&tool.left_calls.to_string(), 6),
            rpad(&format!("-> {}", tool.right_calls), 6),
            rpad(&signed(tool.call_delta()), 7),
            rpad(&thousands(tool.left_tokens), 10),
            rpad(&thousands(tool.right_tokens), 10),
        );
    }

    if diff.tools.len() > LIMIT {
        println!("  ... and {} more tool(s)", diff.tools.len() - LIMIT);
    }
}

pub fn export_ndjson(
    app: &ContextTrace,
    session: &AgentSession,
    resolved: &ResolvedSession,
    estimator: &dyn ct_domain::ports::TokenEstimator,
    redaction: ExportRedaction,
) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;

    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    // Set where the pipe actually broke, because `AppError` flattens an
    // `io::Error` to a string at the port boundary and the kind is the only
    // reliable way to recognise this -- the message differs by platform.
    let mut pipe_closed = false;

    let result = app.export_ndjson(
        session,
        &resolved.descriptor.path,
        resolved.binding,
        estimator,
        redaction,
        |record| {
            let line = serde_json::to_string(record)
                .map_err(|e| AppError::Calibration(format!("could not serialise a record: {e}")))?;
            writeln!(out, "{line}").map_err(|e| {
                pipe_closed |= is_pipe_gone(&e);
                AppError::Port(PortError::Io(e.to_string()))
            })
        },
    );

    match result {
        Ok(report) => {
            if redaction == ExportRedaction::Secrets {
                eprintln!(
                    "Redacted {} potential secret occurrence(s) from exported fields.",
                    report.redactions
                );
            }
        }
        Err(_) if pipe_closed => return Ok(()),
        Err(e) => return Err(e.into()),
    }

    match out.flush() {
        Err(e) if is_pipe_gone(&e) => Ok(()),
        other => Ok(other?),
    }
}

pub fn secrets(session: &AgentSession, report: &SecretScanReport) {
    println!("Potential secrets  {}  {}\n", session.id(), session.agent());
    println!(
        "Scanned {} context-bearing record(s); matched values are never shown or exported.",
        report.scanned_records
    );
    if report.unreadable_records > 0 {
        println!(
            "{} record(s) could not be re-read and were not scanned.",
            report.unreadable_records
        );
    }

    if report.findings.is_empty() {
        println!("\nNo recognised provider credentials found.");
        return;
    }

    println!(
        "\nFound {} potential secret occurrence(s):\n",
        report.occurrence_count()
    );
    for finding in &report.findings {
        let turn = finding
            .turn
            .map(|turn| format!("turn {}", turn.get()))
            .unwrap_or_else(|| "no turn assigned".into());
        println!(
            "  {:<24} {:>3}  {}, line {}  ({})",
            finding.kind.label(),
            finding.occurrences,
            turn,
            finding.line_no,
            finding.event_type
        );
    }
}

/// Has the reader gone away?
///
/// Matched on the error *kind* rather than its text. Windows reports a closed
/// pipe as `ERROR_BROKEN_PIPE` (109) or `ERROR_NO_DATA` (232) without mapping
/// either to `ErrorKind::BrokenPipe`, so a message match would work on one
/// platform and quietly fail on the other -- which is how `ct export <id> |
/// head` came to print an error for doing exactly what was asked.
fn is_pipe_gone(e: &std::io::Error) -> bool {
    const ERROR_BROKEN_PIPE: i32 = 109;
    const ERROR_NO_DATA: i32 = 232;
    e.kind() == std::io::ErrorKind::BrokenPipe
        || matches!(
            e.raw_os_error(),
            Some(ERROR_BROKEN_PIPE) | Some(ERROR_NO_DATA)
        )
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
