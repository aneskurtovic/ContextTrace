//! # ContextTrace CLI
//!
//! The driving adapter and the **composition root** -- the one place in the
//! system that knows which concrete adapters exist. Everything else works
//! through traits, which is what makes a new agent a one-line change here plus
//! a new module in `ct-adapters`.

mod format;
mod render;

use clap::{Parser, Subcommand};
use ct_adapters::{
    ClaudeCodeAdapter, CodexAdapter, FileRawEventSource, HeuristicEstimator, TiktokenEstimator,
};
use ct_application::{AgentBinding, ContextTrace, ResolveError, ResolvedSession, SessionFilter};
use ct_domain::ports::{ExactRecount, TokenEstimator};
use ct_domain::services::DerivedRatio;
use ct_domain::{
    AgentKind, Confidence, ContextCategory, FilterParseError, ItemFilter, SourcePattern, TurnNumber,
};

#[derive(Parser)]
#[command(
    name = "ct",
    version,
    about = "ContextTrace - DevTools for AI coding-agent context",
    long_about = "Inspect what an AI coding agent actually had in its context window, turn by turn.\n\
                  Reads Codex CLI and Claude Code session logs locally. Never writes to them,\n\
                  never uploads anything."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List discovered sessions across all supported agents
    Sessions {
        /// Only this agent: claude-code or codex
        #[arg(long)]
        agent: Option<String>,
        /// Substring match against the project path
        #[arg(long)]
        project: Option<String>,
        /// Only sessions active on or after this date (YYYY-MM-DD)
        #[arg(long)]
        since: Option<String>,
        #[arg(long, default_value_t = 40)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Show a session's timeline and summary
    Inspect {
        /// Session id, or an unambiguous prefix of one
        id: String,
        /// Show the raw JSONL line behind each event
        #[arg(long)]
        raw: bool,
        /// Limit the timeline to this many events
        #[arg(long, default_value_t = 60)]
        limit: usize,
        #[arg(long)]
        json: bool,
    },

    /// Break down the context at a turn by category
    Context {
        id: String,
        /// Which turn (1-based). Defaults to the largest turn in the session.
        #[arg(long)]
        turn: Option<u32>,
        /// Measure item sizes with a real tokenizer instead of estimating them
        ///
        /// Codex only, because only its models have a public tokenizer. Costs a
        /// seek, a JSON parse and a tokenizer pass per item, and covers only the
        /// items whose payload is entirely model-visible text -- roughly three
        /// in five. Deliberately absent from `trace` and `residual`: both sweep
        /// every turn, so the cost multiplies by turn count, and `trace` answers
        /// a membership question that does not depend on the estimator at all.
        #[arg(long)]
        exact: bool,
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long)]
        json: bool,
    },

    /// Show the biggest individual context consumers at a turn
    Largest {
        id: String,
        #[arg(long)]
        turn: Option<u32>,
        #[arg(long, default_value_t = 15)]
        limit: usize,
        /// Measure item sizes with a real tokenizer instead of estimating them
        ///
        /// See `ct context --help`. Most worth using here, because this is the
        /// view that ranks items against each other, and estimation error is
        /// what makes a ranking wrong.
        #[arg(long)]
        exact: bool,
        #[command(flatten)]
        filter: FilterArgs,
        #[arg(long)]
        json: bool,
    },

    /// Follow one context item: when it entered, how long it stayed, what removed it
    ///
    /// `ct context` sees one turn, so it cannot say how long something has been
    /// sitting in the window. This reconstructs every turn and reports where the
    /// item actually appears.
    Trace {
        id: String,
        /// The item: an id like `claude:4821`, or any part of its label
        #[arg(long)]
        item: String,
        #[arg(long)]
        json: bool,
    },

    /// Track the context the agent never logged, turn by turn
    ///
    /// The system prompt and tool schemas do not change while a session runs, so
    /// a step change here means the harness altered them: a tool was registered,
    /// an MCP server connected, a skill loaded.
    Residual {
        id: String,
        /// First turn to report (1-based)
        #[arg(long)]
        from: Option<u32>,
        /// Last turn to report (1-based)
        #[arg(long)]
        to: Option<u32>,
        #[arg(long)]
        json: bool,
    },

    /// Report parse fidelity, context spikes and compactions for a session
    Doctor {
        /// Session id, or an unambiguous prefix of one. Omit it with --dir.
        id: Option<String>,
        /// Sweep every session for event types this build does not recognise
        ///
        /// Give a path to narrow the sweep to sessions under it, or pass the
        /// flag alone to sweep everything `ct roots` lists. Exits non-zero when
        /// anything was not understood, so CI can gate on it: an agent shipping
        /// a new event type is the one change that silently degrades every
        /// other command in this tool.
        #[arg(long, num_args = 0..=1, default_missing_value = "", value_name = "PATH")]
        dir: Option<String>,
        #[arg(long)]
        json: bool,
    },

    /// Stream a whole session as NDJSON, one record per line
    ///
    /// For piping into `jq`, `duckdb` or a script:
    ///
    ///   ct export <id> > session.ndjson
    ///   duckdb -c "SELECT sum(tokens) FROM 'session.ndjson' WHERE type='item'"
    ///
    /// Every item row carries its confidence, and the unattributed remainder is
    /// a row of its own, so summing item tokens per turn agrees with the prompt
    /// size the agent reported. Message previews are excluded: an export leaves
    /// the agent's directory and redaction is not built yet.
    Export {
        id: String,
        #[arg(long, value_enum, default_value_t = ExportFormat::Ndjson)]
        format: ExportFormat,
    },

    /// Show which local directories ContextTrace reads
    Roots,
}

/// Export formats.
///
/// One variant today. It is an enum rather than a bare flag because the entry
/// this came from also asked for Parquet, and adding a second value later must
/// not change the shape of the first invocation.
#[derive(Copy, Clone, PartialEq, Eq, clap::ValueEnum)]
enum ExportFormat {
    Ndjson,
}

/// Narrowing options shared by the two context views.
///
/// Flattened into both commands rather than duplicated, because the whole point
/// is that `ct context --category tool-outputs` and `ct largest --category
/// tool-outputs` select the same rows. Two copies of these arguments would be
/// two chances for them to drift apart.
#[derive(clap::Args, Clone, Debug)]
struct FilterArgs {
    /// Only items with this provenance: `tool`, `tool:Bash`, `file:schema.ts`,
    /// `harness:skill_listing`, `user`, `model`, `system-prompt`
    #[arg(long)]
    source: Option<String>,

    /// Only this category, e.g. `tool-outputs`, `file-contents`, `reasoning`
    #[arg(long)]
    category: Option<String>,

    /// Only items at least this trustworthy: `observed`, `derived`, `estimated`
    #[arg(long)]
    confidence: Option<String>,

    /// Only items of at least this many tokens
    #[arg(long)]
    min_tokens: Option<u32>,
}

impl FilterArgs {
    fn build(&self) -> Result<ItemFilter, Box<dyn std::error::Error>> {
        Ok(ItemFilter {
            source: self.source.as_deref().map(SourcePattern::parse).transpose()?,
            category: self
                .category
                .as_deref()
                .map(|c| {
                    ContextCategory::parse(c).ok_or_else(|| FilterParseError {
                        field: "category",
                        value: c.to_string(),
                        allowed: Vec::new(),
                    })
                })
                .transpose()
                // `ContextCategory::slug` allocates, so the allowed list cannot
                // be built from `&'static str` the way the others are. Rendered
                // here instead of widening the error type for one case.
                .map_err(|e| {
                    let allowed: Vec<String> =
                        ContextCategory::ALL.iter().map(|c| c.slug()).collect();
                    format!(
                        "unknown --category '{}'; expected one of: {}",
                        e.value,
                        allowed.join(", ")
                    )
                })?,
            min_confidence: self
                .confidence
                .as_deref()
                .map(|c| {
                    Confidence::parse(c).ok_or_else(|| FilterParseError {
                        field: "confidence",
                        value: c.to_string(),
                        allowed: Confidence::ALL.iter().map(|c| c.label()).collect(),
                    })
                })
                .transpose()?,
            min_tokens: self.min_tokens,
        })
    }
}

fn main() {
    let cli = Cli::parse();
    let code = match run(cli) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };

    if code != 0 {
        // Flushed explicitly because `exit` runs no destructors, and stdout is
        // block-buffered when piped. Without this, `ct doctor --dir --json | jq`
        // would lose the report it is exiting non-zero *about*.
        use std::io::Write;
        let _ = std::io::stdout().flush();
        std::process::exit(code);
    }
}

/// Wire the concrete adapters. **This function is the composition root.**
///
/// Each agent is paired with the estimator its models call for: `tiktoken`
/// where the tokenizer is public, a heuristic where it is not. The asymmetry is
/// expressed once, here, rather than being rediscovered at call sites.
fn build() -> ContextTrace {
    let codex_estimator: Box<dyn TokenEstimator> = match TiktokenEstimator::o200k() {
        Ok(t) => Box::new(t),
        // Losing the tokenizer degrades accuracy, not function: fall back to the
        // code-density heuristic and carry on, because a session listing should
        // not fail because a BPE table would not load.
        Err(e) => {
            eprintln!("warning: o200k tokenizer unavailable ({e}); falling back to heuristic");
            Box::new(HeuristicEstimator::for_code())
        }
    };

    ContextTrace::new(vec![
        AgentBinding::new(
            Box::new(ClaudeCodeAdapter::new()),
            // Anthropic ships no local tokenizer. Agent sessions are dominated
            // by code and terminal output, so the denser ratio fits better.
            Box::new(HeuristicEstimator::for_code()),
        ),
        AgentBinding::new(Box::new(CodexAdapter::new()), codex_estimator),
    ])
}

fn run(cli: Cli) -> Result<i32, Box<dyn std::error::Error>> {
    let app = build();

    match cli.command {
        Command::Roots => render::roots(&app),

        Command::Sessions {
            agent,
            project,
            since,
            limit,
            json,
        } => {
            let parsed_agent = agent.as_deref().and_then(AgentKind::parse);
            if let Some(raw) = agent.as_deref() {
                if parsed_agent.is_none() {
                    return Err(format!("unknown agent '{raw}'; use claude-code or codex").into());
                }
            }
            let filter = SessionFilter {
                agent: parsed_agent,
                project,
                since: since
                    .as_deref()
                    .map(|s| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d"))
                    .transpose()?,
                limit: Some(limit),
            };
            render::sessions(&app.list_sessions(&filter), json);
        }

        Command::Inspect {
            id,
            raw,
            limit,
            json,
        } => {
            let (session, resolved) = app.load(&id)?;
            render::inspect(&session, &resolved, limit, raw, json)?;
        }

        Command::Context {
            id,
            turn,
            exact,
            filter,
            json,
        } => {
            let filter = filter.build()?;
            let (session, resolved) = app.load(&id)?;
            let turn = pick_turn(&app, &session, turn)?;
            let calibrated = session_estimator(&app, &session, resolved.binding);
            let (snapshot, recount) =
                calibrated.snapshot_for(&app, &session, &resolved, turn, exact)?;
            render::context(
                &snapshot.filtered(&filter),
                &calibrated.name(&app, resolved.binding),
                calibrated.ratio,
                recount,
                json,
            );
        }

        Command::Largest {
            id,
            turn,
            limit,
            exact,
            filter,
            json,
        } => {
            let filter = filter.build()?;
            let (session, resolved) = app.load(&id)?;
            let turn = pick_turn(&app, &session, turn)?;
            let calibrated = session_estimator(&app, &session, resolved.binding);
            let (snapshot, recount) =
                calibrated.snapshot_for(&app, &session, &resolved, turn, exact)?;
            render::largest(
                &snapshot.filtered(&filter),
                calibrated.ratio,
                recount,
                limit,
                json,
            );
        }

        Command::Trace { id, item, json } => {
            let (session, resolved) = app.load(&id)?;
            let sweep = app.sweep_lifecycles(&session, resolved.binding);

            let record = match sweep.resolve(&item) {
                Ok(record) => record,
                // Not a typo to correct but a choice to make: labels are not
                // unique, so the list of candidates *is* the answer, and it
                // carries the ids needed to pick one.
                Err(ResolveError::Ambiguous { needle, candidates }) => {
                    render::trace_candidates(&candidates, json);
                    return Err(format!(
                        "'{needle}' matches {} items; re-run with one of the ids above",
                        candidates.len()
                    )
                    .into());
                }
                Err(e) => return Err(e.into()),
            };

            let life = sweep.lifecycle_of(record);
            // Sized once, at the last turn that held it, through the same
            // calibrated path `ct largest` uses -- see `render::trace` for why
            // a per-turn size series would be misleading rather than richer.
            let calibrated = session_estimator(&app, &session, resolved.binding);
            let size = life
                .last_present()
                .and_then(|t| TurnNumber::new(t).ok())
                .and_then(|turn| {
                    let snapshot = calibrated.snapshot(&app, &session, resolved.binding, turn).ok()?;
                    let contributor = snapshot.contributor(&life.id)?;
                    Some(render::ItemSize {
                        turn: turn.get(),
                        contributor,
                        turn_total: snapshot.total(),
                    })
                });

            render::trace(&life, size.as_ref(), session.agent(), json);
        }

        Command::Residual {
            id,
            from,
            to,
            json,
        } => {
            let (session, resolved) = app.load(&id)?;
            let calibrated = session_estimator(&app, &session, resolved.binding);
            let Some(ratio) = calibrated.ratio else {
                return Err(format!(
                    "cannot measure unlogged context for this session: {}",
                    match session.agent() {
                        AgentKind::Codex =>
                            "no characters-per-token ratio is fitted for Codex sessions, \
                             and this view is built on one",
                        _ => "not enough turn-to-turn growth to derive a ratio",
                    }
                )
                .into());
            };
            let series: Vec<_> = app
                .residual_series(&session, resolved.binding, ratio)
                .into_iter()
                .filter(|p| from.is_none_or(|f| p.turn >= f))
                .filter(|p| to.is_none_or(|t| p.turn <= t))
                .collect();
            let compaction_turns: Vec<u32> = session
                .compactions()
                .iter()
                .filter_map(|(_, e)| e.turn.map(|t| t.get()))
                .collect();
            render::residual(&series, ratio, &compaction_turns, json);
        }

        Command::Export { id, format } => {
            let ExportFormat::Ndjson = format;
            let (session, resolved) = app.load(&id)?;
            render::export_ndjson(&app, &session, &resolved)?;
        }

        Command::Doctor { id, dir, json } => match (id, dir) {
            (Some(_), Some(_)) => {
                return Err("give a session id or --dir, not both: one reports a \
                            session's health, the other sweeps for format drift"
                    .into())
            }
            (None, None) => {
                return Err("give a session id, or --dir to sweep every session for \
                            unrecognised event types"
                    .into())
            }
            (None, Some(prefix)) => {
                let report = app.sweep_drift(Some(prefix.as_str()));
                render::drift(&report, json);
                // The report is the message, so this exits without an `error:`
                // line. CI wants the code; a human wants the histogram.
                //
                // A prefix that matched nothing also fails, and for a reason
                // worth naming: a mistyped --dir would otherwise be a green run
                // claiming an agent's format was checked when nothing was read.
                if !report.is_clean() || report.matched_nothing() {
                    return Ok(1);
                }
            }
            (Some(id), None) => {
                let (session, resolved) = app.load(&id)?;
                render::doctor(&app.diagnose(&session), &session, &resolved, json);
            }
        },
    }

    Ok(0)
}

/// The estimator to use for one session, and the measurement behind it.
///
/// Both are carried together because every consumer needs both: the estimator to
/// produce the numbers, and the [`DerivedRatio`] to know what may honestly be
/// said about them. Splitting them is how `ct largest` came to print a residual
/// captioned "system prompt + tool schemas" for sessions where `ct context`
/// correctly reported the figure as not measurable.
struct SessionCalibration {
    estimator: Option<HeuristicEstimator>,
    ratio: Option<DerivedRatio>,
}

impl SessionCalibration {
    fn snapshot(
        &self,
        app: &ContextTrace,
        session: &ct_domain::AgentSession,
        binding: usize,
        turn: TurnNumber,
    ) -> Result<ct_domain::ContextSnapshot, Box<dyn std::error::Error>> {
        Ok(match &self.estimator {
            Some(e) => app.snapshot_with(session, binding, turn, e)?,
            None => app.snapshot(session, binding, turn)?,
        })
    }

    /// The snapshot behind a single-turn view, exactly counted on request.
    ///
    /// `None` for the recount means exactness was not asked for. That is a
    /// different thing from a recount that measured nothing, and the views have
    /// to be able to tell them apart before they can caption their own numbers.
    fn snapshot_for(
        &self,
        app: &ContextTrace,
        session: &ct_domain::AgentSession,
        resolved: &ResolvedSession,
        turn: TurnNumber,
        exact: bool,
    ) -> Result<(ct_domain::ContextSnapshot, Option<ExactRecount>), Box<dyn std::error::Error>> {
        let binding = resolved.binding;
        if !exact {
            return Ok((self.snapshot(app, session, binding, turn)?, None));
        }

        let raw = FileRawEventSource::for_session(&resolved.descriptor.path);
        let (snapshot, recount) = match &self.estimator {
            Some(e) => app.snapshot_exact_with(session, binding, turn, e, &raw)?,
            None => app.snapshot_exact(session, binding, turn, &raw)?,
        };
        Ok((snapshot, Some(recount)))
    }

    fn name(&self, app: &ContextTrace, binding: usize) -> String {
        match &self.estimator {
            Some(e) => e.name().to_string(),
            None => app.estimator_name(binding).to_string(),
        }
    }
}

/// Build an estimator calibrated to this session, where that makes sense.
///
/// **A composition-root decision, deliberately.** Deriving a characters-per-
/// token ratio fits a curve to how far reconstruction falls short of the
/// observed totals, and that only means something where the shortfall is
/// dominated by estimation error. Codex sessions log their own system prompt,
/// so their shortfall is small and mostly tool schemas -- there is no
/// systematic gap to fit, and `--exact` is the better instrument there anyway.
/// This function is the one place that knows which agent got which estimator,
/// because this function is the one place that paired them.
fn session_estimator(
    app: &ContextTrace,
    session: &ct_domain::AgentSession,
    binding: usize,
) -> SessionCalibration {
    let ratio = (session.agent() == AgentKind::ClaudeCode)
        .then(|| app.derive_ratio(session, binding))
        .flatten();

    SessionCalibration {
        estimator: ratio.map(|r| HeuristicEstimator::with_ratio(r.chars_per_token)),
        ratio,
    }
}

/// Resolve `--turn`, defaulting to the session's largest turn.
///
/// Defaulting to the peak rather than to turn 1 is a deliberate ergonomic
/// choice: someone reaching for this tool is almost always investigating a
/// context problem, and the peak turn is where the problem lives.
fn pick_turn(
    app: &ContextTrace,
    session: &ct_domain::AgentSession,
    requested: Option<u32>,
) -> Result<TurnNumber, Box<dyn std::error::Error>> {
    match requested {
        Some(n) => Ok(TurnNumber::new(n)?),
        None => app
            .peak_turn(session)
            .ok_or_else(|| "this session has no turns with recorded token usage".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use ct_domain::SourceKind;

    fn filter_args(argv: &[&str]) -> FilterArgs {
        match Cli::try_parse_from(argv).expect("must parse").command {
            Command::Context { filter, .. } | Command::Largest { filter, .. } => filter,
            _ => panic!("expected a filtered command"),
        }
    }

    #[test]
    fn the_definition_is_internally_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn both_context_views_accept_the_same_filters() {
        // The reason these arguments are one flattened struct: `ct context` and
        // `ct largest` must select the same rows, and two copies of four
        // arguments is two chances to drift.
        for command in ["context", "largest"] {
            let args = filter_args(&["ct", command, "abc", "--category", "tool-outputs"]);
            let filter = args.build().expect("must build");
            assert_eq!(filter.category, Some(ContextCategory::ToolOutputs));
        }
    }

    #[test]
    fn a_source_argument_carries_its_payload_through() {
        let filter = filter_args(&["ct", "largest", "abc", "--source", "tool:Bash"])
            .build()
            .expect("must build");
        let pattern = filter.source.expect("a source pattern");
        assert_eq!(pattern.kind, SourceKind::Tool);
        assert_eq!(pattern.detail.as_deref(), Some("Bash"));
    }

    #[test]
    fn a_misspelled_value_lists_the_ones_that_work() {
        // A filter that silently matches nothing is indistinguishable from a
        // broken tool, so a value that names nothing has to fail loudly and say
        // what would have worked.
        let err = filter_args(&["ct", "context", "abc", "--category", "tool-output"])
            .build()
            .expect_err("must reject a near-miss")
            .to_string();
        assert!(err.contains("tool-outputs"), "should suggest the real name: {err}");

        let err = filter_args(&["ct", "context", "abc", "--source", "toool"])
            .build()
            .expect_err("must reject an unknown source")
            .to_string();
        assert!(err.contains("compaction-summary"), "should list the kinds: {err}");
    }

    #[test]
    fn no_filter_arguments_means_no_filtering() {
        assert!(!filter_args(&["ct", "context", "abc"]).build().unwrap().is_active());
    }
}
