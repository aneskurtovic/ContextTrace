//! # ContextTrace CLI
//!
//! The driving adapter and the **composition root** -- the one place in the
//! system that knows which concrete adapters exist. Everything else works
//! through traits, which is what makes a new agent a one-line change here plus
//! a new module in `ct-adapters`.

mod format;
mod render;

use clap::{Parser, Subcommand};
use ct_adapters::{ClaudeCodeAdapter, CodexAdapter, HeuristicEstimator, TiktokenEstimator};
use ct_application::{AgentBinding, ContextTrace, SessionFilter};
use ct_domain::ports::TokenEstimator;
use ct_domain::services::DerivedRatio;
use ct_domain::{AgentKind, TurnNumber};

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
        id: String,
        #[arg(long)]
        json: bool,
    },

    /// Show which local directories ContextTrace reads
    Roots,
}

fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
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

fn run(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
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

        Command::Context { id, turn, json } => {
            let (session, resolved) = app.load(&id)?;
            let turn = pick_turn(&app, &session, turn)?;
            let calibrated = session_estimator(&app, &session, resolved.binding);
            let snapshot = calibrated.snapshot(&app, &session, resolved.binding, turn)?;
            render::context(&snapshot, &calibrated.name(&app, resolved.binding), calibrated.ratio, json);
        }

        Command::Largest {
            id,
            turn,
            limit,
            json,
        } => {
            let (session, resolved) = app.load(&id)?;
            let turn = pick_turn(&app, &session, turn)?;
            let calibrated = session_estimator(&app, &session, resolved.binding);
            let snapshot = calibrated.snapshot(&app, &session, resolved.binding, turn)?;
            render::largest(&snapshot, calibrated.ratio, limit, json);
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
                            "Codex items are counted exactly, so there is no fitted \
                             remainder to track",
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

        Command::Doctor { id, json } => {
            let (session, resolved) = app.load(&id)?;
            render::doctor(&app.diagnose(&session), &session, &resolved, json);
        }
    }

    Ok(())
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
/// token ratio is only meaningful where counts are heuristic. Codex runs
/// GPT-family models, so `tiktoken` counts its items exactly and replacing that
/// with a fitted ratio would trade a measurement for an estimate -- strictly
/// worse. This function is the one place that knows which agent got which
/// estimator, because this function is the one place that paired them.
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
