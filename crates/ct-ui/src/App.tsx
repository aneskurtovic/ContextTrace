import { useCallback, useEffect, useRef, useState } from "react";
import * as api from "./api";
import {
  errorMessage,
  formatActivity,
  formatBytes,
  formatPercent,
  formatTokens,
  projectName,
  shortId,
} from "./format";
import type {
  Agent,
  ArchiveEntrySummary,
  ArchiveHolding,
  ArchiveVerification,
  Comparability,
  CompactionDiff,
  CompactionDiffItem,
  CompactionDiffUnavailableReason,
  CompactionItemDisposition,
  ContextDetail,
  CostReport,
  DoctorReport,
  ExportOutcome,
  GrowthPoint,
  InstructionFileReport,
  LifecycleReport,
  ResidualPoint,
  ResidualReport,
  ResidualStep,
  SessionDetail,
  SessionSummary,
  StartupSummary,
  TurnDiff,
  TurnTarget,
  TemporalGhost,
} from "./types";

type AgentFilter = "all" | Agent;

/**
 * A session's id alone is not unique across agents, so selection, the
 * currently-loaded detail, and dedup all carry the agent alongside the id.
 */
type SessionKey = { agent: Agent; id: string };

function sameSession(a: SessionKey | null, b: SessionKey | null): boolean {
  return a != null && b != null && a.agent === b.agent && a.id === b.id;
}

/**
 * Whether two comparison targets name the same turn of the same session.
 *
 * The turn-comparison panel used to guard against comparing a turn with
 * itself by checking turn numbers alone, because both sides were always the
 * currently-open session. That guard would now misfire the moment the right
 * side is a *different* session: turn 5 of session A against turn 5 of
 * session B is a legitimate request, not a self-comparison. Identity has to
 * include the session, not just the turn.
 */
function sameTarget(a: TurnTarget | null, b: TurnTarget | null): boolean {
  return a != null && b != null && a.agent === b.agent && a.id === b.id && a.turn === b.turn;
}

function AgentMark({ agent }: { agent: Agent }) {
  const label = agent === "codex" ? "Codex" : "Claude Code";
  return (
    <span className={`agent-mark ${agent}`} aria-label={label}>
      {agent === "codex" ? "CX" : "CC"}
    </span>
  );
}

function Spinner({ label }: { label: string }) {
  return (
    <div className="loading" role="status" aria-live="polite" aria-atomic="true">
      <span className="spinner" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}

function Metric({
  label,
  value,
  note,
  accent,
}: {
  label: string;
  value: string;
  note: string;
  accent?: boolean;
}) {
  return (
    <div className={`metric ${accent ? "metric-accent" : ""}`}>
      <span className="metric-label">{label}</span>
      <strong>{value}</strong>
      <span className="metric-note">{note}</span>
    </div>
  );
}

function SessionListItem({
  session,
  selected,
  onSelect,
}: {
  session: SessionSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  // Narrowed through the value itself rather than a boolean alias, so the
  // union tells the compiler `parent` is a string in this branch.
  const role = session.threadRole;
  return (
    <button
      className={`session-row ${selected ? "selected" : ""}`}
      onClick={onSelect}
      aria-current={selected ? "true" : undefined}
      aria-label={`${agentLabel(session.agent)} session: ${projectName(session.project)}, ${shortId(session.id)}${
        role.kind === "subagent" ? `, subagent of ${shortId(role.parent)}` : ""
      }`}
    >
      <AgentMark agent={session.agent} />
      <span className="session-copy">
        <span className="session-title">{projectName(session.project)}</span>
        <span className="session-meta">
          {shortId(session.id)} · {formatBytes(session.sizeBytes)}
          {role.kind === "subagent" && (
            <span className="thread-marker"> · subagent of {shortId(role.parent)}</span>
          )}
        </span>
      </span>
      <span className="session-activity">{formatActivity(session.lastActivity)}</span>
    </button>
  );
}

function signed(value: number): string {
  return `${value > 0 ? "+" : value < 0 ? "−" : ""}${Math.abs(value).toLocaleString()}`;
}

/**
 * What the instruments alone permit, said before any delta is read.
 *
 * Each arm states a different thing, because `Comparability` does: an exact
 * subtraction, one bounded by a stated skew, or one that cannot be performed.
 * The third case deliberately does not fall back to showing token deltas
 * anyway — counts still mean something when scales do not, so those are what
 * it points the reader at.
 *
 * `identical` claims one *instrument*, never one session: two different
 * Codex sessions both report through the same named tokenizer with nothing
 * fitted per session, so a genuine cross-session pair can land here too. The
 * side labels beside each turn, not this note, are what say whether the two
 * turns came from the same session.
 */
function ComparabilityNote({ comparability }: { comparability: Comparability }) {
  if (comparability.kind === "identical") {
    return (
      <p className="diff-instrument">
        <strong>{comparability.estimator}</strong> sized both turns — one instrument, so
        every delta below is content.
      </p>
    );
  }
  if (comparability.kind === "skewed") {
    return (
      <p className="diff-instrument diff-instrument-warn">
        <strong>{comparability.left}</strong> vs <strong>{comparability.right}</strong> —{" "}
        {(comparability.skew * 100).toFixed(1)}% apart. The two sides are reported on
        differently graduated scales; each row states how much of its delta that alone
        could explain.
      </p>
    );
  }
  return (
    <p className="diff-instrument diff-instrument-refusal">
      <strong>{comparability.left}</strong> vs <strong>{comparability.right}</strong>. No
      token delta is reported: {comparability.reason}. The item and call counts are
      unaffected, and they are the comparison.
    </p>
  );
}

/** A turn side, labelled with which session it belongs to -- never just a
 *  turn number, now that the two sides may be different sessions. */
function sideLabel(side: { agent: Agent; id: string; turn: number }): string {
  return `${agentLabel(side.agent)} ${shortId(side.id)} turn ${side.turn}`;
}

/**
 * The right-hand session picker for the turn comparison.
 *
 * Encodes each option as `JSON.stringify({agent,id})` rather than a
 * delimited string: session ids are opaque and some observed on real
 * machines carry punctuation, so building a compound key by concatenation
 * risks two different sessions parsing back to the same pair.
 */
function CrossSessionPicker({
  sessions,
  leftTarget,
  crossSession,
  crossTurnInput,
  onSetCrossSession,
  onSetCrossTurnInput,
}: {
  sessions: SessionSummary[];
  leftTarget: TurnTarget;
  crossSession: { agent: Agent; id: string } | null;
  crossTurnInput: string;
  onSetCrossSession: (target: { agent: Agent; id: string } | null) => void;
  onSetCrossTurnInput: (value: string) => void;
}) {
  const otherSessions = sessions.filter(
    (session) => !(session.agent === leftTarget.agent && session.id === leftTarget.id),
  );
  return (
    <div className="diff-cross-session">
      <label>
        <span>Compare this turn with</span>
        <select
          value={crossSession ? JSON.stringify(crossSession) : ""}
          onChange={(event) => {
            if (!event.target.value) {
              onSetCrossSession(null);
              return;
            }
            onSetCrossSession(JSON.parse(event.target.value));
          }}
        >
          <option value="">Another turn in this session</option>
          {otherSessions.map((session) => (
            <option
              key={`${session.agent}:${session.id}`}
              value={JSON.stringify({ agent: session.agent, id: session.id })}
            >
              {agentLabel(session.agent)} · {projectName(session.project)} · {shortId(session.id)}
            </option>
          ))}
        </select>
      </label>
      {crossSession && (
        <label>
          <span>Turn to compare</span>
          <input
            type="number"
            min={1}
            value={crossTurnInput}
            onChange={(event) => onSetCrossTurnInput(event.target.value)}
            aria-label={`Turn to compare against in ${agentLabel(crossSession.agent)} ${shortId(crossSession.id)}`}
          />
        </label>
      )}
    </div>
  );
}

function TurnComparison({
  diff,
  loading,
  leftTarget,
  rightTarget,
  sessions,
  crossSession,
  crossTurnInput,
  onSetCrossSession,
  onSetCrossTurnInput,
}: {
  diff: TurnDiff | null;
  loading: boolean;
  leftTarget: TurnTarget | null;
  rightTarget: TurnTarget | null;
  sessions: SessionSummary[];
  crossSession: { agent: Agent; id: string } | null;
  crossTurnInput: string;
  onSetCrossSession: (target: { agent: Agent; id: string } | null) => void;
  onSetCrossTurnInput: (value: string) => void;
}) {
  if (leftTarget == null) return null;

  const picker = (
    <CrossSessionPicker
      sessions={sessions}
      leftTarget={leftTarget}
      crossSession={crossSession}
      crossTurnInput={crossTurnInput}
      onSetCrossSession={onSetCrossSession}
      onSetCrossTurnInput={onSetCrossTurnInput}
    />
  );

  const noComparisonPicked = rightTarget == null || sameTarget(leftTarget, rightTarget);
  if (noComparisonPicked) {
    return (
      <section className="panel diff-panel" aria-labelledby="diff-heading">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Turn comparison</span>
          <h2 id="diff-heading">Baseline pinned at {sideLabel(leftTarget)}</h2>
          </div>
        </div>
        {picker}
        <p className="diff-empty">
          {crossSession
            ? "Enter a turn number to compare against in the selected session."
            : "Pin a baseline, then choose another turn on the timeline or compare with a different session above."}
        </p>
      </section>
    );
  }
  if (loading && !diff) {
    return (
      <section className="panel diff-panel" aria-labelledby="diff-heading">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Turn comparison</span>
            <h2 id="diff-heading">Comparing turns…</h2>
          </div>
        </div>
        {picker}
        <Spinner label="Comparing turns…" />
      </section>
    );
  }
  if (!diff) return null;

  const tokensComparable = diff.comparability.kind !== "incomparable";
  return (
    <section className="panel diff-panel" aria-labelledby="diff-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Turn comparison</span>
          <h2 id="diff-heading">
            {sideLabel(diff.left)} → {sideLabel(diff.right)}
          </h2>
        </div>
        {diff.totalsAreObserved && (
          <span className="panel-total">
            {signed(diff.promptDelta)} tokens reported
          </span>
        )}
      </div>

      {picker}

      <ComparabilityNote comparability={diff.comparability} />

      <div className="doctor-section">
        <div className="doctor-section-title">
          <strong>Categories</strong>
          <span>largest change first</span>
        </div>
        {diff.categories.length === 0 ? (
          <p className="diff-empty">No category appears on either side.</p>
        ) : (
          diff.categories.map((row) => (
            <div className="doctor-row" key={row.category}>
              <div className="doctor-row-copy">
                <span className="doctor-row-label">{row.category}</span>
                <small>
                  {row.left.toLocaleString()} → {row.right.toLocaleString()} ·{" "}
                  {signed(row.itemDelta)} items
                  {row.instrumentBound != null && row.instrumentBound > 0 && (
                    <> · ±{row.instrumentBound.toLocaleString()} from instruments alone</>
                  )}
                </small>
              </div>
              <span
                className={
                  tokensComparable && row.meaningful
                    ? "doctor-row-value"
                    : "doctor-row-value muted"
                }
                title={
                  tokensComparable
                    ? row.meaningful
                      ? "Larger than the instruments could explain"
                      : "Within what the instruments alone could produce"
                    : "No scale relates the two sides, so this delta is not reported"
                }
              >
                {tokensComparable ? signed(row.delta) : "—"}
              </span>
            </div>
          ))
        )}
      </div>

      {diff.tools.length > 0 && (
        <div className="doctor-section">
          <div className="doctor-section-title">
            <strong>Tools</strong>
            <span>calls are counts, so no instrument distorts them</span>
          </div>
          {diff.tools.map((row) => (
            <div className="doctor-row" key={row.tool}>
              <div className="doctor-row-copy">
                <span className="doctor-row-label">{row.tool}</span>
                <small>
                  {row.leftCalls} → {row.rightCalls} calls · {signed(row.callDelta)}
                </small>
              </div>
              <span
                className={
                  tokensComparable && row.meaningful
                    ? "doctor-row-value"
                    : "doctor-row-value muted"
                }
              >
                {tokensComparable ? signed(row.tokenDelta) : "—"}
              </span>
            </div>
          ))}
        </div>
      )}

      <p className="compaction-note">
        {diff.totalsAreObserved
          ? "The headline change is read from both turns' own usage records, so it carries no instrument caveat. Category and tool rows are reconstructed and do."
          : "At least one side's total was not reported by the agent, so the headline change is withheld; the rows below are reconstructed."}
      </p>
    </section>
  );
}

function agentLabel(agent: Agent) {
  return agent === "codex" ? "Codex" : "Claude Code";
}

function GrowthChart({
  points,
  selectedTurn,
  onTurn,
  selectedCompactionLineNo,
  onCompaction,
}: {
  points: GrowthPoint[];
  selectedTurn: number | null;
  onTurn: (turn: number) => void;
  selectedCompactionLineNo: number | null;
  onCompaction: (lineNo: number) => void;
}) {
  const measured = points.filter(
    (point): point is GrowthPoint & { promptTokens: number } =>
      point.promptTokens != null,
  );
  if (measured.length < 2) {
    return <p className="chart-empty">Not enough measured turns to chart.</p>;
  }

  const width = 900;
  const height = 180;
  const padX = 8;
  const padY = 14;
  const max = Math.max(...measured.map((point) => point.promptTokens));
  const minTurn = measured[0].turn;
  const maxTurn = measured[measured.length - 1].turn;
  const x = (turn: number) =>
    padX + ((turn - minTurn) / Math.max(1, maxTurn - minTurn)) * (width - padX * 2);
  const y = (tokens: number) =>
    height - padY - (tokens / Math.max(1, max)) * (height - padY * 2);
  const line = measured
    .map((point, index) => `${index ? "L" : "M"} ${x(point.turn)} ${y(point.promptTokens)}`)
    .join(" ");
  const area = `${line} L ${x(maxTurn)} ${height - padY} L ${x(minTurn)} ${height - padY} Z`;

  return (
    <div className="chart-wrap">
      <svg
        className="growth-chart"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="Prompt tokens across measured turns"
      >
        <defs>
          <linearGradient id="growth-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0%" stopColor="#4f8cff" stopOpacity=".34" />
            <stop offset="100%" stopColor="#4f8cff" stopOpacity=".015" />
          </linearGradient>
        </defs>
        <line className="grid-line" x1="0" y1={height * 0.33} x2={width} y2={height * 0.33} />
        <line className="grid-line" x1="0" y1={height * 0.66} x2={width} y2={height * 0.66} />
        <path d={area} fill="url(#growth-fill)" />
        <path d={line} className="growth-line" />
        {points
          .filter((point): point is GrowthPoint & { compaction: NonNullable<GrowthPoint["compaction"]> } =>
            point.compaction != null,
          )
          .map((point) => {
            const compaction = point.compaction;
            const isOpen = compaction.lineNo === selectedCompactionLineNo;
            const reclaimedLabel =
              compaction.reclaimed != null
                ? `, reclaiming ${compaction.reclaimed.toLocaleString()} tokens`
                : "";
            return (
              <g key={`compaction-${point.turn}-${compaction.lineNo}`}>
                <line
                  className={isOpen ? "compaction-line selected" : "compaction-line"}
                  x1={x(point.turn)}
                  x2={x(point.turn)}
                  y1={10}
                  y2={height - 10}
                />
                <line
                  className={isOpen ? "compaction-hit selected" : "compaction-hit"}
                  x1={x(point.turn)}
                  x2={x(point.turn)}
                  y1={10}
                  y2={height - 10}
                  role="button"
                  tabIndex={0}
                  aria-pressed={isOpen}
                  aria-label={`Inspect compaction at turn ${point.turn}${reclaimedLabel}`}
                  onClick={() => onCompaction(compaction.lineNo)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      onCompaction(compaction.lineNo);
                    }
                  }}
                />
              </g>
            );
          })}
        {measured.map((point) => (
          <circle
            key={point.turn}
            className={point.turn === selectedTurn ? "chart-point selected" : "chart-point"}
            cx={x(point.turn)}
            cy={y(point.promptTokens)}
            r={point.turn === selectedTurn ? 5 : 2.5}
            onClick={() => onTurn(point.turn)}
            role="button"
            tabIndex={0}
            aria-pressed={point.turn === selectedTurn}
            aria-label={`Inspect turn ${point.turn}: ${point.promptTokens.toLocaleString()} prompt tokens`}
            onKeyDown={(event) => {
              if (event.key === "Enter" || event.key === " ") {
                event.preventDefault();
                onTurn(point.turn);
              }
            }}
          />
        ))}
      </svg>
      <div className="chart-axis">
        <span>Turn {minTurn}</span>
        <span>{formatTokens(max)} peak</span>
        <span>Turn {maxTurn}</span>
      </div>
    </div>
  );
}

/**
 * The unlogged remainder across a session's turns.
 *
 * Turns whose remainder is unknown break the line rather than dropping to the
 * axis. A zero there would read as "nothing hidden at this turn", which is the
 * opposite of what an over-count means, so the gap is drawn as a gap and
 * marked.
 */
function ResidualChart({
  points,
  steps,
}: {
  points: ResidualPoint[];
  steps: ResidualStep[];
}) {
  const known = points.filter(
    (point): point is ResidualPoint & { unlogged: number } => point.unlogged != null,
  );
  if (known.length < 2 || points.length < 2) {
    return (
      <p className="chart-empty">
        Fewer than two turns have a readable remainder — too few to chart.
      </p>
    );
  }

  const width = 900;
  const height = 180;
  const padX = 8;
  const padY = 14;
  const max = Math.max(...known.map((point) => point.unlogged));
  const minTurn = points[0].turn;
  const maxTurn = points[points.length - 1].turn;
  const x = (turn: number) =>
    padX + ((turn - minTurn) / Math.max(1, maxTurn - minTurn)) * (width - padX * 2);
  const y = (tokens: number) =>
    height - padY - (tokens / Math.max(1, max)) * (height - padY * 2);

  // One path per unbroken run of measured turns. Joining across a gap would
  // draw a line through a turn that reported no remainder at all.
  const segments: string[] = [];
  let run: string[] = [];
  for (const point of points) {
    if (point.unlogged == null) {
      if (run.length > 1) segments.push(run.join(" "));
      run = [];
      continue;
    }
    run.push(`${run.length ? "L" : "M"} ${x(point.turn)} ${y(point.unlogged)}`);
  }
  if (run.length > 1) segments.push(run.join(" "));

  return (
    <div className="chart-wrap">
      <svg
        className="growth-chart"
        viewBox={`0 0 ${width} ${height}`}
        role="img"
        aria-label="Unlogged context per turn"
      >
        <line className="grid-line" x1="0" y1={height * 0.33} x2={width} y2={height * 0.33} />
        <line className="grid-line" x1="0" y1={height * 0.66} x2={width} y2={height * 0.66} />
        {steps.map((step) => (
          <line
            key={`step-${step.turn}`}
            className={step.nearCompaction ? "residual-step explained" : "residual-step"}
            x1={x(step.turn)}
            x2={x(step.turn)}
            y1={10}
            y2={height - 10}
          />
        ))}
        {points
          .filter((point) => point.unlogged == null)
          .map((point) => (
            <line
              key={`gap-${point.turn}`}
              className="residual-gap"
              x1={x(point.turn)}
              x2={x(point.turn)}
              y1={10}
              y2={height - 10}
            />
          ))}
        {segments.map((segment) => (
          <path key={segment.slice(0, 24)} d={segment} className="residual-line" />
        ))}
        {known.map((point) => (
          <circle
            key={point.turn}
            className="chart-point"
            cx={x(point.turn)}
            cy={y(point.unlogged)}
            r={2.5}
          >
            <title>
              {`Turn ${point.turn}: ${point.unlogged.toLocaleString()} unlogged of ${point.promptTokens.toLocaleString()} prompt tokens`}
            </title>
          </circle>
        ))}
      </svg>
      <div className="chart-axis">
        <span>Turn {minTurn}</span>
        <span>{formatTokens(max)} peak remainder</span>
        <span>Turn {maxTurn}</span>
      </div>
    </div>
  );
}

/**
 * What a step means, said only where the log does not already say it.
 *
 * A compaction rewrites the whole prompt, so a step beside one has a cause
 * already recorded. Narrating it as an unrecorded harness change would invent a
 * second explanation for an event the session accounts for — which is why this
 * mirrors the terminal view's gating rather than captioning every step.
 */
function ResidualSteps({ steps }: { steps: ResidualStep[] }) {
  if (steps.length === 0) {
    return (
      <p className="doctor-clean-state">
        No sustained change in the unlogged remainder. Nothing suggests the harness altered
        this session's hidden context while it ran.
      </p>
    );
  }
  const anyUnexplained = steps.some((step) => !step.nearCompaction);

  return (
    <div className="doctor-section">
      <div className="doctor-section-title">
        <strong>Sustained changes</strong>
        <span>Median of the five turns either side, so drift does not qualify</span>
      </div>
      {steps.map((step) => (
        <div className="doctor-row" key={`step-row-${step.turn}`}>
          <div className="doctor-row-copy">
            <strong>Turn {step.turn}</strong>
            <span>
              {formatTokens(step.from)} → {formatTokens(step.to)}
            </span>
            <small>
              {step.nearCompaction
                ? "A compaction occurred here, which explains it."
                : "Nothing in the log records a change here."}
            </small>
          </div>
          <div className="doctor-row-number">
            <strong className={step.growth > 0 ? "residual-rise" : "residual-fall"}>
              {signed(step.growth)}
            </strong>
            <span>tokens</span>
          </div>
        </div>
      ))}
      {anyUnexplained && (
        <p className="doctor-more">
          A rise means the prompt gained content the log does not record — a tool registered, an
          MCP server connected, a skill loaded. A fall means reconstruction began accounting for
          more of the prompt than before, which is either hidden content going away or
          over-counting. This view cannot tell those two apart. Read a step as evidence that
          something changed, not as its size.
        </p>
      )}
    </div>
  );
}

/**
 * The context an agent never wrote down, across a whole session.
 *
 * User-triggered like the doctor scan, and for the same reason: producing the
 * series reconstructs every turn, which is not a cost to pay on every click
 * through the session list.
 */
function UnloggedContext({
  report,
  loading,
  onRun,
}: {
  report: ResidualReport | null;
  loading: boolean;
  onRun: () => void;
}) {
  return (
    <section className="panel doctor-panel" aria-labelledby="residual-heading" aria-busy={loading}>
      <div className="panel-heading doctor-heading">
        <div>
          <span className="eyebrow">Unlogged context</span>
          <h2 id="residual-heading">What is missing from the log</h2>
        </div>
        <button className="doctor-run" onClick={onRun} disabled={loading}>
          {loading ? "Measuring locally…" : report ? "Measure again" : "Measure this session"}
        </button>
      </div>

      {!report && !loading && (
        <p className="doctor-intro">
          A prompt is larger than everything the log records. The difference is the system
          prompt and tool schemas the agent never wrote down, recovered by fitting this
          session's own characters-per-token ratio to its usage figures. Reconstructing every
          turn takes a moment, so it runs when you ask.
        </p>
      )}
      {loading && <Spinner label="Reconstructing every turn to fit this session's ratio…" />}

      {report?.kind === "agentNotFitted" && !loading && (
        <p className="doctor-warning">
          No ratio is fitted for {agentLabel(report.agent)}, and this measurement is built on
          one. {agentLabel(report.agent)} records its own system prompt and has a public
          tokenizer, so its remaining gap is not a ratio to fit — there is no remainder here to
          recover, rather than one that could not be read.
        </p>
      )}

      {report?.kind === "insufficientGrowth" && !loading && (
        <p className="doctor-warning">
          The ratio is measured from turn-to-turn growth, and this session did not grow enough
          to measure one — {report.turnsWithUsage} turn(s) reported prompt usage. Nothing is
          estimated in its place, because a guessed ratio would return noise rather than a
          remainder.
        </p>
      )}

      {report?.kind === "overCounted" && !loading && (
        <>
          <p className="doctor-warning">
            A ratio fitted at {report.charsPerToken.toFixed(2)} characters per token across{" "}
            {report.pairsUsed} turn pairs, but all {report.turnsMeasured} turns reconstruct to
            more content than their prompts held. Every remainder comes out negative, so no
            chart is drawn.
          </p>
          <p className="doctor-more">
            This happens on roughly one Claude Code session in seven and its cause is not
            established. The measurement is refused rather than clamped to zero, which would
            claim a complete inventory of the context.
          </p>
        </>
      )}

      {report?.kind === "fitted" && !loading && (
        <>
          <div className="doctor-summary" aria-live="polite">
            <div>
              <span>Characters per token</span>
              <strong>{report.charsPerToken.toFixed(2)}</strong>
              <small>
                {report.pairsUsed} turn pairs · spread {report.dispersion.toFixed(2)}×
              </small>
            </div>
            <div>
              <span>Typical hidden constant</span>
              <strong>
                {report.unloggedOverhead == null
                  ? "not measurable"
                  : formatTokens(report.unloggedOverhead)}
              </strong>
              <small>{report.remainderConfidence} · prompts {report.promptConfidence}</small>
            </div>
            <div className={report.overCountedTurns ? "doctor-alert" : "doctor-clean"}>
              <span>Turns over-counted</span>
              <strong>
                {report.overCountedTurns} of {report.turnsMeasured}
              </strong>
              <small>
                {report.overCountedTurns
                  ? "remainder unknown, drawn as a gap"
                  : "every turn had a readable remainder"}
              </small>
            </div>
          </div>

          <div className="legend residual-legend">
            <span><i className="legend-residual" /> unlogged remainder</span>
            <span><i className="legend-step" /> sustained change</span>
            <span><i className="legend-step legend-step-explained" /> explained by a compaction</span>
            {report.overCountedTurns > 0 && (
              <span><i className="legend-gap" /> over-counted turn</span>
            )}
          </div>

          <ResidualChart points={report.points} steps={report.steps} />

          <p className="doctor-more">
            This line drifts: one fitted ratio cannot describe a session that starts as prose
            and ends dominated by tool output, and whatever the ratio gets wrong lands here.
            Read the trend, not the turn-to-turn wiggle — only changes of at least{" "}
            {formatTokens(report.stepThreshold)} tokens that hold are marked.
          </p>

          <ResidualSteps steps={report.steps} />
        </>
      )}
    </section>
  );
}

function ContextComposition({ context }: { context: ContextDetail }) {
  const categories = Array.isArray(context.categories) ? context.categories : [];
  return (
    <section className="panel composition-panel" aria-labelledby="composition-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Composition</span>
          <h2 id="composition-heading">What filled the context window</h2>
        </div>
        <span className="panel-total">{context.totalTokens.toLocaleString()} tokens</span>
      </div>
      <div className="composition-list">
        {categories.length ? (
          categories.map((category) => (
            <div className="composition-row" key={category.category}>
              <div className="composition-label">
                <span>{category.label}</span>
                <small>
                  {category.itemCount} {category.itemCount === 1 ? "item" : "items"} ·{" "}
                  {category.confidence}
                </small>
              </div>
              <div className="composition-bar-track">
                <span
                  className={`composition-bar category-${category.category}`}
                  style={{ width: `${Math.max(category.share * 100, 0.8)}%` }}
                />
              </div>
              <div className="composition-value">
                <strong>{formatTokens(category.tokens)}</strong>
                <span>{formatPercent(category.share)}</span>
              </div>
            </div>
          ))
        ) : (
          <p className="empty-inline">No context categories were reported for this turn.</p>
        )}
      </div>
      {categories.length > 0 && (
        <div className="context-treemap" aria-label="Context category treemap">
          {categories.map((category) => (
            <div
              className={`context-treemap-cell category-${category.category}`}
              key={`map-${category.category}`}
              style={{
                gridColumn: `span ${Math.max(1, Math.round(category.share * 12))}`,
                minHeight: `${Math.max(42, Math.round(42 + category.share * 72))}px`,
              }}
              title={`${category.label}: ${formatTokens(category.tokens)} tokens (${formatPercent(category.share)})`}
            >
              <strong>{category.label}</strong>
              <span>{formatTokens(category.tokens)} · {formatPercent(category.share)}</span>
            </div>
          ))}
        </div>
      )}
      <p className="callout">
        <span aria-hidden="true">i</span>
        Confidence labels: observed = logged, derived = reconstructed, estimated = modelled.
        {context.calibrationScale != null && (
          <> Estimated items calibrated {context.calibrationScale.toFixed(2)}× to the reported total.</>
        )}
      </p>
      {context.residualIsMeaningful ? (
        context.residualTokens > 0 ? (
          <p className="callout">
            <span aria-hidden="true">?</span>
            {context.residualTokens.toLocaleString()} tokens are present in the reported total but
            not attributable from the session log—usually hidden system instructions, tool
            schemas, or request framing.
          </p>
        ) : (
          <p className="callout">
            <span aria-hidden="true">✓</span>
            Unattributed remainder: 0 tokens for this reconstruction.
          </p>
        )
      ) : (
        <p className="callout">
          <span aria-hidden="true">?</span>
          Unattributed remainder: not measurable. Rows are proportions of the reported total, not
          a complete inventory.
        </p>
      )}
    </section>
  );
}

function Contributors({
  context,
  selectedItem,
  onSelect,
}: {
  context: ContextDetail;
  selectedItem: string | null;
  onSelect: (item: string) => void;
}) {
  const contributors = Array.isArray(context.contributors) ? context.contributors : [];
  return (
    <section className="panel contributors-panel" aria-labelledby="contributors-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Largest contributors</span>
          <h2 id="contributors-heading">Where the context went</h2>
        </div>
        <span className="count-pill">{contributors.length} shown</span>
      </div>
      <div className="contributor-list">
        {contributors.length ? (
          contributors.map((item, index) => (
            <button
              type="button"
              className={selectedItem === item.id ? "contributor-row selected" : "contributor-row"}
              key={item.id}
              onClick={() => onSelect(item.id)}
              aria-expanded={selectedItem === item.id}
              aria-controls="item-lifecycle"
              title={`Trace ${item.label} through the session`}
            >
              <span className="rank">{String(index + 1).padStart(2, "0")}</span>
              <div className="contributor-copy">
                <strong title={item.label}>{item.label}</strong>
                <span>
                  {item.category} · {item.source}
                </span>
              </div>
              <span className={`confidence confidence-${item.confidence}`}>
                {item.confidence}
              </span>
              <div className="contributor-number">
                <strong>{formatTokens(item.tokens)}</strong>
                <span>{formatPercent(item.share)}</span>
              </div>
            </button>
          ))
        ) : (
          <p className="empty-inline">No individual contributors were reported for this turn.</p>
        )}
      </div>
    </section>
  );
}

function departureText(report: LifecycleReport): string {
  const departure = report.departure;
  if (!departure) {
    return report.stillPresent
      ? `Still present at the last scanned turn (${report.lastScannedTurn ?? "unknown"}).`
      : "No departure can be established from the readable turns.";
  }
  if (departure.kind === "compaction") {
    const reclaimed = departure.reclaimed
      ? `, which reclaimed ${formatTokens(departure.reclaimed)}`
      : "";
    return `Removed by a recorded compaction${departure.turn ? ` at turn ${departure.turn}` : ""}${reclaimed}.`;
  }
  if (departure.kind === "branch-diverged") {
    return `The conversation moved to a different Claude Code branch at turn ${departure.turn}; this was not an eviction.`;
  }
  return `The item disappeared before turn ${departure.turn}, but the log records no cause.`;
}

/**
 * "history #N" and "replacement #N -> replacement #N" match `ct compactions`'
 * own labelling verbatim (see render.rs's `compactions()`), including the
 * footer note below: the CLI already solved this labelling problem, so the
 * desktop must not invent a second vocabulary for the same two lists.
 */
function compactionPositionLabel(disposition: CompactionItemDisposition): string {
  switch (disposition.kind) {
    case "dropped":
      return `history #${disposition.historyIndex}`;
    case "preserved":
      return `history #${disposition.historyIndex} → replacement #${disposition.replacementIndex}`;
    case "addedByReplacement":
      return `replacement #${disposition.replacementIndex}`;
  }
}

function compactionUnavailableText(reason: CompactionDiffUnavailableReason): string {
  switch (reason) {
    case "missingReplacementHistory":
      return "This compaction's log line carries no replacement history, so there is nothing to diff.";
    case "oversizedRawLine":
      return "The raw log line behind this compaction is larger than this build parses, so its replacement history could not be read.";
    case "unavailableRawLine":
      return "The log line behind this compaction could not be read from disk.";
    case "malformedRawLine":
      return "The log line behind this compaction is not valid JSON.";
    case "malformedPrecedingItem":
      return "An item in the history immediately before this compaction could not be parsed, so the two lists cannot be compared.";
    case "unknownPrecedingHistory":
      return "An earlier compaction in this session could not be read, so the history this one replaced is not known.";
  }
}

function CompactionRow({ item }: { item: CompactionDiffItem }) {
  return (
    <div className="doctor-row compaction-row">
      <div className="doctor-row-copy">
        <strong>
          {item.itemType}
          {item.role ? ` · ${item.role}` : ""}
        </strong>
        <span>{compactionPositionLabel(item.disposition)}</span>
        <small className={`confidence confidence-${item.confidence}`}>{item.confidence}</small>
      </div>
      <div className="doctor-row-number">
        <strong>{formatBytes(item.normalizedJsonBytes)}</strong>
        <span>{item.textTokens != null ? `${formatTokens(item.textTokens)} tokens` : "opaque / structured"}</span>
      </div>
    </div>
  );
}

function CompactionGroup({
  title,
  note,
  items,
}: {
  title: string;
  note: string;
  items: CompactionDiffItem[];
}) {
  if (items.length === 0) return null;
  return (
    <div className="doctor-section compaction-group">
      <div className="doctor-section-title">
        <strong>{title}</strong>
        <span>{note}</span>
      </div>
      {items.map((item, index) => (
        <CompactionRow item={item} key={`${title}-${index}-${item.itemType}`} />
      ))}
    </div>
  );
}

/** Groups items by disposition so the three outcomes -- dropped, preserved,
 *  added by replacement -- are distinguishable at a glance, per CT-047. */
function CompactionGroups({ items }: { items: CompactionDiffItem[] }) {
  const dropped = items.filter((item) => item.disposition.kind === "dropped");
  const preserved = items.filter((item) => item.disposition.kind === "preserved");
  const added = items.filter((item) => item.disposition.kind === "addedByReplacement");
  return (
    <>
      <div className="doctor-summary compaction-summary" aria-live="polite">
        <div>
          <span>Dropped</span>
          <strong>{dropped.length}</strong>
          <small>present only in history</small>
        </div>
        <div>
          <span>Preserved</span>
          <strong>{preserved.length}</strong>
          <small>carried into the replacement</small>
        </div>
        <div>
          <span>Added by replacement</span>
          <strong>{added.length}</strong>
          <small>no identical predecessor</small>
        </div>
      </div>
      <CompactionGroup
        title="Dropped"
        note="Present only in the pre-compaction history"
        items={dropped}
      />
      <CompactionGroup
        title="Preserved"
        note="Present in both lists; the arrow shows where it moved to"
        items={preserved}
      />
      <CompactionGroup
        title="Added by replacement"
        note="Introduced by the replacement, with no identical predecessor"
        items={added}
      />
      <p className="compaction-note">
        Sizes are normalized compact item bytes [derived]; text tokens are measured only for
        wholly textual items. "history #N" indexes the pre-compaction history; "replacement #N"
        indexes the replacement — the two lists are numbered separately.
      </p>
    </>
  );
}

function CompactionAutopsy({
  diff,
  loading,
  onClose,
}: {
  diff: CompactionDiff | null;
  loading: boolean;
  onClose: () => void;
}) {
  if (!diff && !loading) return null;
  const heading =
    diff?.status === "available"
      ? `What turn ${diff.turn ?? "?"}'s compaction replaced`
      : diff?.status === "unavailable"
        ? `Compaction at turn ${diff.turn ?? "?"}: evidence unavailable`
        : "Compaction autopsy unavailable";
  return (
    <section
      className="panel compaction-panel"
      id="compaction-autopsy"
      aria-labelledby="compaction-heading"
      aria-busy={loading}
    >
      {loading && !diff ? (
        <Spinner label="Reading the compaction's replacement history…" />
      ) : diff ? (
        <>
          <div className="panel-heading compaction-heading">
            <div>
              <span className="eyebrow">Compaction autopsy</span>
              <h2 id="compaction-heading">{heading}</h2>
            </div>
            <button className="compaction-close" onClick={onClose} aria-label="Close compaction autopsy">×</button>
          </div>
          {diff.status === "unsupported" && (
            <p className="compaction-refusal">
              This agent does not record a literal replacement history for its compactions, so
              there is no diff to show. Showing one anyway would misrepresent evidence this tool
              does not have.
              <br />
              <small>{diff.detail}</small>
            </p>
          )}
          {diff.status === "unavailable" && (
            <p className="compaction-refusal">{compactionUnavailableText(diff.reason)}</p>
          )}
          {diff.status === "available" && <CompactionGroups items={diff.items} />}
        </>
      ) : null}
    </section>
  );
}

function LifecyclePanel({
  report,
  loading,
  onClose,
}: {
  report: LifecycleReport | null;
  loading: boolean;
  onClose: () => void;
}) {
  if (!report && !loading) return null;
  return (
    <section className="panel lifecycle-panel" id="item-lifecycle" aria-labelledby="lifecycle-heading" aria-busy={loading}>
      {loading && !report ? (
        <Spinner label="Tracing this item across every turn…" />
      ) : report ? (
        <>
          <div className="panel-heading lifecycle-heading">
            <div>
              <span className="eyebrow">Item lifecycle</span>
              <h2 id="lifecycle-heading" title={report.label}>{report.label}</h2>
              <p>{report.category} · {report.source}</p>
            </div>
            <button className="lifecycle-close" onClick={onClose} aria-label="Close item lifecycle">×</button>
          </div>
          <div className="lifecycle-metrics">
            <div><span>First in prompt</span><strong>{report.firstPresent ?? "—"}</strong></div>
            <div><span>Last in prompt</span><strong>{report.lastPresent ?? "—"}</strong></div>
            <div><span>Turns present</span><strong>{report.turnsPresent}</strong></div>
            <div><span>Runs</span><strong>{report.runs.length}</strong></div>
          </div>
          <div className="lifecycle-runs" aria-label="Observed presence ranges">
            {report.runs.map((run) => (
              <span key={`${run.from}-${run.to}`}>
                {run.from === run.to ? `turn ${run.from}` : `turns ${run.from}–${run.to}`}
                <small>{run.turns} observed</small>
              </span>
            ))}
          </div>
          <p className={`lifecycle-departure ${report.departure?.kind ?? "present"}`}>
            {departureText(report)}
          </p>
          {report.firstSeenDisagrees && (
            <p className="lifecycle-note">
              The log wrote this item at turn {report.recordedFirstSeen}, while reconstruction first observed it in a prompt at turn {report.firstPresent}. Both facts are preserved.
            </p>
          )}
          {report.unknownTurns.length > 0 && (
            <p className="lifecycle-note">
              Presence is unknown at unreadable turn(s): {report.unknownTurns.join(", ")}.
            </p>
          )}
          {report.otherThreadTurns > 0 && (
            <p className="lifecycle-note">
              {report.otherThreadTurns} turn(s) on the other main/subagent context were excluded.
            </p>
          )}
        </>
      ) : null}
    </section>
  );
}

function ContextDoctor({
  turn,
  report,
  loading,
  onRun,
}: {
  turn: number;
  report: DoctorReport | null;
  loading: boolean;
  onRun: () => void;
}) {
  const clean =
    report &&
    report.duplicateGroups === 0 &&
    report.lowEntropyItems === 0 &&
    report.secretOccurrences === 0;

  return (
    <section className="panel doctor-panel" aria-labelledby="doctor-heading" aria-busy={loading}>
      <div className="panel-heading doctor-heading">
        <div>
          <span className="eyebrow">Context Doctor</span>
          <h2 id="doctor-heading">Find waste and possible secrets</h2>
        </div>
        <button className="doctor-run" onClick={onRun} disabled={loading}>
          {loading ? "Scanning locally…" : report ? "Scan again" : `Analyze turn ${turn}`}
        </button>
      </div>

      {!report && !loading && (
        <p className="doctor-intro">
          Opt in to a deeper local read. ContextTrace fingerprints and compresses model-visible
          records, then scans credential shapes without returning their values. Nothing leaves
          this computer.
        </p>
      )}
      {loading && <Spinner label="Fingerprinting, compressing, and checking local records…" />}
      {report && !loading && (
        <>
          <div className="doctor-summary" aria-live="polite">
            <div>
              <span>Exact repeats</span>
              <strong>{formatTokens(report.repeatedTokens)}</strong>
              <small>{report.duplicateGroups} duplicate group(s)</small>
            </div>
            <div>
              <span>Low-information score</span>
              <strong>{formatTokens(report.wasteScoreTokens)}</strong>
              <small>{report.lowEntropyItems} qualifying block(s)</small>
            </div>
            <div className={report.secretOccurrences ? "doctor-alert" : "doctor-clean"}>
              <span>Potential secrets</span>
              <strong>{report.secretOccurrences}</strong>
              <small>{report.scannedRecords} record(s) checked</small>
            </div>
          </div>

          {clean && (
            <p className="doctor-clean-state">
              No exact repeats, qualifying low-information blocks, or recognised credential
              shapes were found for this view.
            </p>
          )}

          {report.duplicates.length > 0 && (
            <div className="doctor-section">
              <div className="doctor-section-title">
                <strong>Repeated content</strong>
                <span>Exact model-visible matches · turn {report.turn}</span>
              </div>
              {report.duplicates.map((finding, index) => (
                <div className="doctor-row" key={`duplicate-${index}-${finding.items[0]?.label}`}>
                  <div className="doctor-row-copy">
                    <strong title={finding.items[0]?.label}>{finding.items[0]?.label}</strong>
                    <span>
                      {finding.copies} copies · {formatTokens(finding.totalTokens)} total · {formatPercent(finding.share)} of prompt
                    </span>
                    <small title={finding.items.map((item) => item.source).join(" · ")}>
                      {finding.items.map((item) => item.source).join(" · ")}
                    </small>
                  </div>
                  <div className="doctor-row-number">
                    <strong>{formatTokens(finding.repeatedTokens)}</strong>
                    <span>repeated</span>
                  </div>
                </div>
              ))}
              {report.duplicateGroups > report.duplicates.length && (
                <p className="doctor-more">
                  {report.duplicateGroups - report.duplicates.length} smaller duplicate group(s) included in the total.
                </p>
              )}
            </div>
          )}

          {report.lowEntropy.length > 0 && (
            <div className="doctor-section">
              <div className="doctor-section-title">
                <strong>Low-information blocks</strong>
                <span>Large payloads compressed to 75% or less</span>
              </div>
              {report.lowEntropy.map((finding) => (
                <div className="doctor-row" key={`entropy-${finding.label}-${finding.source}`}>
                  <div className="doctor-row-copy">
                    <strong title={finding.label}>{finding.label}</strong>
                    <span>{finding.source} · {formatTokens(finding.tokens)} · {formatPercent(finding.share)} of prompt</span>
                    <small>{formatPercent(finding.compressionRatio)} compressed/original; score is a ranking, not a savings claim</small>
                  </div>
                  <div className="doctor-row-number">
                    <strong>{formatTokens(finding.wasteScoreTokens)}</strong>
                    <span>waste score</span>
                  </div>
                </div>
              ))}
              {report.lowEntropyItems > report.lowEntropy.length && (
                <p className="doctor-more">
                  {report.lowEntropyItems - report.lowEntropy.length} smaller block(s) included in the total.
                </p>
              )}
            </div>
          )}

          {report.secrets.length > 0 && (
            <div className="doctor-section secret-section">
              <div className="doctor-section-title">
                <strong>Potential credentials</strong>
                <span>Values are deliberately never returned</span>
              </div>
              {report.secrets.map((finding, index) => (
                <div className="secret-row" key={`${finding.line}-${finding.kind}-${index}`}>
                  <strong>{finding.kind}</strong>
                  <span>{finding.occurrences} occurrence(s)</span>
                  <code>line {finding.line}{finding.turn ? ` · turn ${finding.turn}` : ""} · {finding.eventType}</code>
                </div>
              ))}
              {report.secretFindings > report.secrets.length && (
                <p className="doctor-more">
                  {report.secretFindings - report.secrets.length} additional location(s) included in the total.
                </p>
              )}
            </div>
          )}

          {report.unreadableRecords > 0 && (
            <p className="doctor-warning">
              {report.unreadableRecords} context-bearing record(s) could not be read; the secret scan is incomplete.
            </p>
          )}

          {report.unmeasuredItems > 0 && (
            <p className="doctor-warning">
              {report.unmeasuredItems} item(s) had no content measurement and were not checked for duplicates or low-information content.
            </p>
          )}
        </>
      )}
    </section>
  );
}

/** How this row's redaction reads to someone glancing down the list: the
 *  default named plainly, the opt-in named as the unusual choice it is. */
function redactionLabel(redaction: ArchiveEntrySummary["redaction"]): string {
  return redaction === "raw" ? "Raw — credentials kept" : "Redacted";
}

/**
 * What one verification found, rendered as the four genuinely different
 * situations `ArchiveIntegritySummary` is -- never collapsed to a red/green
 * badge. `sourceGone` is the case archiving exists for, and reads as
 * important information rather than as an error: the source is gone, and
 * this copy either still proves itself or it does not, and both outcomes get
 * their own sentence rather than one badge trying to cover both.
 */
function ArchiveIntegrityNote({ verification }: { verification: ArchiveVerification }) {
  const integrity = verification.integrity;
  if (integrity.kind === "intact") {
    return (
      <p className="callout archive-note">
        <span aria-hidden="true">✓</span>
        Matches its source exactly. Nothing to do.
      </p>
    );
  }
  if (integrity.kind === "sourceChanged") {
    return (
      <p className="callout archive-note">
        <span aria-hidden="true">i</span>
        The live log has grown since this copy was made ({formatBytes(integrity.recordedBytes)} →{" "}
        {formatBytes(integrity.currentBytes)}) — the session kept going. Re-archive to bring the
        copy current.
      </p>
    );
  }
  if (integrity.kind === "sourceGone") {
    return (
      <p
        className={
          verification.copyIsSound
            ? "callout archive-note archive-important"
            : "callout archive-note archive-alert"
        }
      >
        <span aria-hidden="true">{verification.copyIsSound ? "◆" : "!"}</span>
        {verification.copyIsSound
          ? "The source log is gone. This copy is the only evidence left, and it still matches what was written — nothing can rebuild it, so this is what there is."
          : "The source log is gone, and this copy no longer matches what was written either. Treat it as unreliable; nothing here can recover the original."}
      </p>
    );
  }
  return (
    <p className="callout archive-note archive-alert">
      <span aria-hidden="true">!</span>
      This archived file no longer matches its recorded digest. Whatever it says now, it is not
      what was archived. Re-archive from the source if it is still present.
    </p>
  );
}

/** One archived session. Deliberately not a button: see `ArchivePanel` for
 *  why a clickable row here would be worse than doing nothing at all. */
function ArchiveEntryRow({
  entry,
  verification,
  onVerify,
}: {
  entry: ArchiveEntrySummary;
  verification: ArchiveVerification | "loading" | Error | undefined;
  onVerify: () => void;
}) {
  return (
    <div className="archive-row" role="listitem">
      <div className="archive-row-copy">
        <div className="archive-row-title">
          <AgentMark agent={entry.agent} />
          <strong title={entry.project ?? undefined}>{projectName(entry.project)}</strong>
          <span className={entry.redaction === "raw" ? "redaction-badge redaction-raw" : "redaction-badge"}>
            {redactionLabel(entry.redaction)}
          </span>
        </div>
        <span className="archive-row-meta">
          {shortId(entry.id)} · archived {formatActivity(entry.archivedAt)} ·{" "}
          {entry.records.toLocaleString()} record(s) · {formatBytes(entry.archivedBytes)}
        </span>
        {entry.differsFromSource && (
          <small className="archive-differs">
            {entry.redactedValues.toLocaleString()} value(s) replaced — this copy is not
            byte-identical to its source.
          </small>
        )}
      </div>
      <div className="archive-row-actions">
        <button type="button" onClick={onVerify} disabled={verification === "loading"}>
          {verification === "loading" ? "Verifying…" : "Verify"}
        </button>
      </div>
      {verification &&
        verification !== "loading" &&
        (verification instanceof Error ? (
          <p className="callout archive-note archive-alert">
            <span aria-hidden="true">!</span>
            {verification.message}
          </p>
        ) : (
          <ArchiveIntegrityNote verification={verification} />
        ))}
    </div>
  );
}

/**
 * What the archive holds, and the one control that adds to it.
 *
 * Rows are deliberately not clickable: in a GUI a list this shape reads as
 * "open on click", and nothing can read an archived session back yet -- that
 * is a separate, not-yet-built capability. A row that silently did nothing
 * would make this worse than the CLI on the same capability; a row that
 * opened the *live* session in its place would misrepresent a copy as the
 * thing itself. So the affordance is stated in words, and the only
 * interactive element per row is the explicit "Verify" action.
 *
 * `holding.root` is shown unconditionally: it is the first directory this
 * tool ever writes to, and the app's privacy claim rests on naming it. The
 * startup panel's roots disclosure also names it (as the written entry
 * alongside the read ones), so a user who never opens a session still sees
 * it -- this panel repeats it for whoever is already looking here.
 */
function ArchivePanel({
  holding,
  loading,
  selected,
  archiving,
  archiveRaw,
  onToggleRaw,
  onArchive,
  verifications,
  onVerify,
  demoData,
}: {
  holding: ArchiveHolding | null;
  loading: boolean;
  selected: SessionSummary;
  archiving: boolean;
  archiveRaw: boolean;
  onToggleRaw: (raw: boolean) => void;
  onArchive: () => void;
  verifications: Record<string, ArchiveVerification | "loading" | Error>;
  onVerify: (agent: Agent, id: string) => void;
  demoData: boolean;
}) {
  return (
    <section className="panel archive-panel" aria-labelledby="archive-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Archive</span>
          <h2 id="archive-heading">Saved copies of this session</h2>
        </div>
        {holding && <span className="panel-total">{holding.entries.length} held</span>}
      </div>

      <p className="archive-root">
        Written to <code>{holding ? holding.root : "…"}</code> — the one directory ContextTrace
        writes to.
      </p>

      <p className="callout archive-affordance">
        <span aria-hidden="true">i</span>
        These rows do not open a session. Nothing reads a copy back yet — here or in the CLI —
        so a copy taken now is insurance against losing the log, not a second way to read it.
        <code>Verify</code> re-checks a copy against its source without opening either.
      </p>

      <div className="archive-add">
        <label className="archive-raw-toggle">
          <input
            type="checkbox"
            checked={archiveRaw}
            onChange={(event) => onToggleRaw(event.target.checked)}
          />
          Keep credentials instead of redacting them — not the usual choice
        </label>
        <button type="button" onClick={onArchive} disabled={archiving || demoData}>
          {archiving ? "Archiving…" : `Add ${projectName(selected.project)} to the archive`}
        </button>
      </div>
      {demoData && (
        <p className="callout">
          <span aria-hidden="true">i</span>
          Demo mode has nothing to write. Run the desktop app against local logs to archive a
          session.
        </p>
      )}

      {loading && !holding ? (
        <Spinner label="Reading the archive…" />
      ) : holding && holding.entries.length ? (
        <div className="archive-list" role="list" aria-label="Archived sessions">
          {holding.entries.map((entry) => (
            <ArchiveEntryRow
              key={`${entry.agent}:${entry.id}`}
              entry={entry}
              verification={verifications[`${entry.agent}:${entry.id}`]}
              onVerify={() => onVerify(entry.agent, entry.id)}
            />
          ))}
        </div>
      ) : (
        <p className="empty-inline">Nothing has been archived yet.</p>
      )}
    </section>
  );
}

/**
 * Writing one session out as NDJSON, and reporting exactly what was written.
 *
 * Defaults to redaction, which `ct export` does not, because the two write to
 * different places. The CLI writes to stdout: the user picks the destination
 * in the same breath as the command, and often it is a pipe that never
 * becomes a file. This writes a durable file into a ContextTrace-owned
 * directory that sits beside the archive -- so the argument that made the
 * archive redact by default applies here unchanged, and having one
 * subdirectory of that root default to redacted while its sibling defaulted
 * to raw would be a distinction no one could hold in their head.
 *
 * The checkbox therefore turns redaction *off*, and says what that leaves in
 * the file rather than merely naming the flag.
 */
function ExportControl({
  exporting,
  outcome,
  error,
  archiveRoot,
  keepSecrets,
  onToggleKeep,
  onExport,
  demoData,
}: {
  exporting: boolean;
  outcome: ExportOutcome | null;
  error: string | null;
  archiveRoot: string | null;
  keepSecrets: boolean;
  onToggleKeep: (keep: boolean) => void;
  onExport: () => void;
  demoData: boolean;
}) {
  return (
    <section className="panel export-panel" aria-labelledby="export-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Export</span>
          <h2 id="export-heading">Export this session</h2>
        </div>
      </div>

      {archiveRoot && (
        <p className="archive-root">
          Written under <code>{archiveRoot}\exports</code> — inside the one directory ContextTrace
          writes to, not a second one.
        </p>
      )}

      <label className="export-toggle">
        <input
          type="checkbox"
          checked={keepSecrets}
          onChange={(event) => onToggleKeep(event.target.checked)}
        />
        Keep recognised credentials in the file instead of replacing them — not the usual choice
      </label>
      <button type="button" onClick={onExport} disabled={exporting || demoData}>
        {exporting ? "Exporting…" : "Export to NDJSON"}
      </button>
      {demoData && (
        <p className="callout">
          <span aria-hidden="true">i</span>
          Demo mode has nothing to write. Run the desktop app against local logs to export a
          session.
        </p>
      )}
      {error && (
        <p className="callout archive-note archive-alert">
          <span aria-hidden="true">!</span>
          {error}
        </p>
      )}
      {outcome && (
        <dl className="export-outcome">
          <div>
            <dt>Written to</dt>
            <dd>
              <code>{outcome.path}</code>
            </dd>
          </div>
          <div>
            <dt>Size</dt>
            <dd>{formatBytes(outcome.bytes)}</dd>
          </div>
          <div>
            <dt>Records</dt>
            <dd>{outcome.records.toLocaleString()}</dd>
          </div>
          <div>
            <dt>Credentials</dt>
            <dd>
              {outcome.redaction === "secrets"
                ? `scanned — ${outcome.redactions.toLocaleString()} value(s) replaced`
                : "kept — this file holds whatever the log held"}
            </dd>
          </div>
        </dl>
      )}
    </section>
  );
}

function EvidenceTools({
  instructionFiles,
  instructionFilesLoading,
  onRunInstructionFiles,
  ghost,
  ghostLoading,
  onRunGhost,
  currentTurn,
  pinnedTurn,
  cost,
  costLoading,
  onRunCost,
}: {
  instructionFiles: InstructionFileReport | null;
  instructionFilesLoading: boolean;
  onRunInstructionFiles: () => void;
  ghost: TemporalGhost | null;
  ghostLoading: boolean;
  onRunGhost: () => void;
  currentTurn: number | null;
  pinnedTurn: number | null;
  cost: CostReport | null;
  costLoading: boolean;
  onRunCost: (pricingPath: string | null, forecastTurns: number | null) => void;
}) {
  const [pricingPath, setPricingPath] = useState("");
  const [forecastTurns, setForecastTurns] = useState("");
  return (
    <section className="panel evidence-tools" aria-labelledby="evidence-tools-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Investigate</span>
          <h2 id="evidence-tools-heading">Investigate what changed</h2>
        </div>
        <div className="button-row">
          <button type="button" onClick={onRunInstructionFiles} disabled={instructionFilesLoading}>
            {instructionFilesLoading ? "Checking files…" : "Check instruction files"}
          </button>
          <button
            type="button"
            onClick={onRunGhost}
            disabled={ghostLoading || pinnedTurn == null || currentTurn == null}
          >
            {ghostLoading ? "Reconstructing…" : "Find hidden changes"}
          </button>
        </div>
      </div>
      {instructionFiles && (
        <div className="evidence-result">
          <strong>Instruction files</strong>
          {instructionFiles.comparisons.length === 0 ? (
            <p>No repository instruction files were recorded.</p>
          ) : (
            instructionFiles.comparisons.map((comparison) => (
              <div className="evidence-row" key={`${comparison.path}:${comparison.line}`}>
                <span className={`status-dot ${comparison.status}`} />
                <code>{comparison.path}</code>
                <span>{comparison.status}</span>
                <small>{comparison.detail}</small>
              </div>
            ))
          )}
        </div>
      )}
      {ghost && (
        <div className="evidence-result">
          <strong>Temporal ghost</strong>
          {ghost.status === "unavailable" ? (
            <p>{ghost.reason}</p>
          ) : (
            <>
              <p>
                Turn {ghost.leftTurn} → {ghost.rightTurn}: {ghost.gained.length} gained, {ghost.retained.length} retained, {ghost.removed.length} removed.
              </p>
              <div className="ghost-columns">
                {(["gained", "removed"] as const).map((group) => (
                  <div key={group}>
                    <span className="eyebrow">{group}</span>
                    {ghost[group].slice(0, 8).map((item) => (
                      <div className="ghost-item" key={`${group}:${item.id}`}>
                        {item.label} <small>{item.confidence}</small>
                      </div>
                    ))}
                  </div>
                ))}
              </div>
            </>
          )}
        </div>
      )}
      <div className="evidence-result">
        <strong>Estimate future cost</strong>
        <div className="cost-controls">
          <label>
            Pricing file <span className="field-help">Optional; blank uses built-in rates.</span>
            <input
              value={pricingPath}
              onChange={(event) => setPricingPath(event.target.value)}
              placeholder="e.g. C:\\path\\pricing.json"
            />
          </label>
          <label>
            More turns <span className="field-help">How many future turns should we model?</span>
            <input
              type="text"
              inputMode="numeric"
              aria-label="Additional forecast turns"
              value={forecastTurns}
              placeholder="e.g. 10"
              onChange={(event) => setForecastTurns(event.target.value)}
            />
          </label>
          <button
            type="button"
            disabled={costLoading}
            onClick={() => onRunCost(pricingPath.trim() || null, Number(forecastTurns) || null)}
          >
            {costLoading ? "Estimating…" : "Estimate"}
          </button>
        </div>
        {cost && (
          <p>
            {cost.pricingSource}: ${(cost.total / 1_000_000).toFixed(6)} observed
            {cost.forecast &&
              ` · $${(cost.forecast.projectedTotal / 1_000_000).toFixed(6)} projected`}
          </p>
        )}
      </div>
    </section>
  );
}

function SessionWorkspace({
  detail,
  context,
  contextLoading,
  doctor,
  doctorLoading,
  lifecycle,
  lifecycleLoading,
  lifecycleItem,
  compactionDiff,
  compactionLoading,
  compactionLineNo,
  turnDiff,
  turnDiffLoading,
  pinnedTurn,
  onTogglePin,
  sessions,
  leftTarget,
  rightTarget,
  crossSession,
  crossTurnInput,
  onSetCrossSession,
  onSetCrossTurnInput,
  residual,
  residualLoading,
  onRunResidual,
  demoData,
  archive,
  archiveLoading,
  archiving,
  archiveRaw,
  onToggleArchiveRaw,
  onArchiveSelected,
  verifications,
  onVerifyEntry,
  exporting,
  exportOutcome,
  exportError,
  exportKeepSecrets,
  onToggleExportKeep,
  onExportSelected,
  onContributor,
  onCloseLifecycle,
  onRunDoctor,
  onTurn,
  onCompaction,
  onCloseCompaction,
  instructionFiles,
  instructionFilesLoading,
  onRunInstructionFiles,
  ghost,
  ghostLoading,
  onRunGhost,
  cost,
  costLoading,
  onRunCost,
}: {
  detail: SessionDetail;
  context: ContextDetail | null;
  contextLoading: boolean;
  doctor: DoctorReport | null;
  doctorLoading: boolean;
  lifecycle: LifecycleReport | null;
  lifecycleLoading: boolean;
  lifecycleItem: string | null;
  compactionDiff: CompactionDiff | null;
  compactionLoading: boolean;
  compactionLineNo: number | null;
  turnDiff: TurnDiff | null;
  turnDiffLoading: boolean;
  pinnedTurn: number | null;
  onTogglePin: () => void;
  sessions: SessionSummary[];
  leftTarget: TurnTarget | null;
  rightTarget: TurnTarget | null;
  crossSession: { agent: Agent; id: string } | null;
  crossTurnInput: string;
  onSetCrossSession: (target: { agent: Agent; id: string } | null) => void;
  onSetCrossTurnInput: (value: string) => void;
  residual: ResidualReport | null;
  residualLoading: boolean;
  onRunResidual: () => void;
  demoData: boolean;
  archive: ArchiveHolding | null;
  archiveLoading: boolean;
  archiving: boolean;
  archiveRaw: boolean;
  onToggleArchiveRaw: (raw: boolean) => void;
  onArchiveSelected: () => void;
  verifications: Record<string, ArchiveVerification | "loading" | Error>;
  onVerifyEntry: (agent: Agent, id: string) => void;
  exporting: boolean;
  exportOutcome: ExportOutcome | null;
  exportError: string | null;
  exportKeepSecrets: boolean;
  onToggleExportKeep: (keep: boolean) => void;
  onExportSelected: () => void;
  onContributor: (item: string) => void;
  onCloseLifecycle: () => void;
  onRunDoctor: () => void;
  onTurn: (turn: number) => void;
  onCompaction: (lineNo: number) => void;
  onCloseCompaction: () => void;
  instructionFiles: InstructionFileReport | null;
  instructionFilesLoading: boolean;
  onRunInstructionFiles: () => void;
  ghost: TemporalGhost | null;
  ghostLoading: boolean;
  onRunGhost: () => void;
  cost: CostReport | null;
  costLoading: boolean;
  onRunCost: (pricingPath: string | null, forecastTurns: number | null) => void;
}) {
  const growth = Array.isArray(detail.growth) ? detail.growth : [];
  const measuredTurns = growth.filter((point) => point.promptTokens != null);
  const selectedIndex = Math.max(
    0,
    measuredTurns.findIndex((point) => point.turn === context?.turn),
  );
  const peakUtilisation =
    detail.peakPromptTokens && detail.contextWindow
      ? detail.peakPromptTokens / detail.contextWindow
      : null;

  return (
    <main className="workspace" aria-busy={contextLoading}>
      <div id="overview" className="workspace-section">
      <header className="workspace-header">
        <div>
          <div className="title-line">
            <AgentMark agent={detail.session.agent} />
            <h1>{projectName(detail.session.project)}</h1>
          </div>
          <p className="path" title={detail.session.project ?? detail.session.path}>
            {detail.session.project ?? detail.session.path}
          </p>
          <div className="session-tags">
            <span>{detail.model ?? "Model not recorded"}</span>
            {detail.gitBranch && <span>branch: {detail.gitBranch}</span>}
            <span>{shortId(detail.session.id)}</span>
            {detail.session.threadRole.kind === "subagent" && (
              <span>subagent of {shortId(detail.session.threadRole.parent)}</span>
            )}
            {detail.source?.kind === "archive" && (
              <span>
                archive copy
                {detail.source.differsFromSource ? " · redacted" : ""}
              </span>
            )}
          </div>
        </div>
        {demoData ? (
          <div className="privacy-badge demo-badge">
            <span className="demo-dot" />
            Demo data
          </div>
        ) : (
          <div className="privacy-badge">
            <span className="privacy-dot" />
            Local only
          </div>
        )}
      </header>

      <section className="metrics">
        <Metric
          label="Largest prompt"
          value={formatTokens(detail.peakPromptTokens)}
          note={detail.peakTurn ? `at turn ${detail.peakTurn}` : "No usage recorded"}
          accent
        />
        <Metric
          label="Context used"
          value={formatPercent(peakUtilisation)}
          note={
            detail.contextWindow
              ? `${formatTokens(detail.contextWindow)} token window`
              : "Window not reported"
          }
        />
        <Metric
          label="Turns"
          value={`${detail.turnCount}`}
          note={`${detail.eventCount.toLocaleString()} events`}
        />
        <Metric
          label="Data coverage"
          value={formatPercent(detail.fidelity)}
          note={
            detail.unrecognisedEvents
              ? `${detail.unrecognisedEvents} unrecognised`
              : "Every event understood"
          }
        />
      </section>

      <section className="panel timeline-panel" aria-labelledby="timeline-heading">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Context over time</span>
            <h2 id="timeline-heading">How this session grew</h2>
          </div>
          <div className="legend">
            <span><i className="legend-growth" /> prompt size</span>
            <span><i className="legend-compaction" /> compaction</span>
          </div>
        </div>
        <GrowthChart
          points={growth}
          selectedTurn={context?.turn ?? detail.peakTurn}
          onTurn={onTurn}
          selectedCompactionLineNo={compactionLineNo}
          onCompaction={onCompaction}
        />
        {measuredTurns.length > 0 && (
          <div className="turn-control">
            <label htmlFor="turn-range">
              Inspect turn <strong>{context?.turn ?? detail.peakTurn ?? "—"}</strong>
            </label>
            <input
              id="turn-range"
              type="range"
              min={0}
              max={Math.max(0, measuredTurns.length - 1)}
              value={selectedIndex}
              onChange={(event) => onTurn(measuredTurns[Number(event.target.value)].turn)}
            />
            <span>
              <output aria-live="polite">
                {context ? `${context.totalTokens.toLocaleString()} tokens` : "Loading context"}
              </output>
            </span>
            <button
              type="button"
              className={pinnedTurn == null ? "pin-turn" : "pin-turn pinned"}
              onClick={onTogglePin}
              aria-pressed={pinnedTurn != null}
              aria-label={
                pinnedTurn == null
                  ? "Pin this turn as the comparison baseline"
                  : `Unpin turn ${pinnedTurn}, the comparison baseline`
              }
            >
              {pinnedTurn == null ? "Pin as baseline" : `Baseline: turn ${pinnedTurn} ✕`}
            </button>
          </div>
        )}
      </section>
      </div>

      <div id="compare" className="workspace-section">
      <TurnComparison
        diff={turnDiff}
        loading={turnDiffLoading}
        leftTarget={leftTarget}
        rightTarget={rightTarget}
        sessions={sessions}
        crossSession={crossSession}
        crossTurnInput={crossTurnInput}
        onSetCrossSession={onSetCrossSession}
        onSetCrossTurnInput={onSetCrossTurnInput}
      />

      <CompactionAutopsy
        diff={compactionDiff}
        loading={compactionLoading}
        onClose={onCloseCompaction}
      />
      </div>

      <div id="insights" className="workspace-section">
      <UnloggedContext report={residual} loading={residualLoading} onRun={onRunResidual} />

      <EvidenceTools
        instructionFiles={instructionFiles}
        instructionFilesLoading={instructionFilesLoading}
        onRunInstructionFiles={onRunInstructionFiles}
        ghost={ghost}
        ghostLoading={ghostLoading}
        onRunGhost={onRunGhost}
        currentTurn={context?.turn ?? null}
        pinnedTurn={pinnedTurn}
        cost={cost}
        costLoading={costLoading}
        onRunCost={onRunCost}
      />

      {contextLoading && !context ? (
        <Spinner label="Reconstructing context…" />
      ) : context ? (
        <>
          <div className={contextLoading ? "context-grid refreshing" : "context-grid"}>
            <ContextComposition context={context} />
            <Contributors
              context={context}
              selectedItem={lifecycleItem}
              onSelect={onContributor}
            />
          </div>
          <LifecyclePanel
            report={lifecycle}
            loading={lifecycleLoading}
            onClose={onCloseLifecycle}
          />
          <ContextDoctor
            turn={context.turn}
            report={doctor?.turn === context.turn ? doctor : null}
            loading={doctorLoading}
            onRun={onRunDoctor}
          />
        </>
      ) : (
        <p className="empty-inline">This session has no reconstructable prompt turn.</p>
      )}
      </div>

      <div id="archive" className="workspace-section">
        <ArchivePanel
          holding={archive}
          loading={archiveLoading}
          selected={detail.session}
          archiving={archiving}
          archiveRaw={archiveRaw}
          onToggleRaw={onToggleArchiveRaw}
          onArchive={onArchiveSelected}
          verifications={verifications}
          onVerify={onVerifyEntry}
          demoData={demoData}
        />

        <ExportControl
          exporting={exporting}
          outcome={exportOutcome}
          error={exportError}
          archiveRoot={archive?.root ?? null}
          keepSecrets={exportKeepSecrets}
          onToggleKeep={onToggleExportKeep}
          onExport={onExportSelected}
          demoData={demoData}
        />
      </div>
    </main>
  );
}

export default function App() {
  // Without the desktop bridge every panel below is filled from `demo.ts`.
  // A tool that argues for evidence over invention cannot render invented
  // sessions in the same chrome as a real read, so this drives a label on
  // every surface those figures reach.
  const demoData = api.isDemoData();
  const [startup, setStartup] = useState<StartupSummary | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [sessionTotal, setSessionTotal] = useState(0);
  const [hasMoreSessions, setHasMoreSessions] = useState(false);
  const [selected, setSelected] = useState<SessionKey | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [detailFor, setDetailFor] = useState<SessionKey | null>(null);
  const [context, setContext] = useState<ContextDetail | null>(null);
  const [agentFilter, setAgentFilter] = useState<AgentFilter>("all");
  const [query, setQuery] = useState("");
  const [debouncedQuery, setDebouncedQuery] = useState("");
  const [loadingSessions, setLoadingSessions] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [loadingContext, setLoadingContext] = useState(false);
  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [loadingDoctor, setLoadingDoctor] = useState(false);
  const [lifecycle, setLifecycle] = useState<LifecycleReport | null>(null);
  const [lifecycleItem, setLifecycleItem] = useState<string | null>(null);
  const [loadingLifecycle, setLoadingLifecycle] = useState(false);
  const [compactionDiff, setCompactionDiff] = useState<CompactionDiff | null>(null);
  const [compactionLineNo, setCompactionLineNo] = useState<number | null>(null);
  const [loadingCompaction, setLoadingCompaction] = useState(false);
  const [pinnedTurn, setPinnedTurn] = useState<number | null>(null);
  const [turnDiff, setTurnDiff] = useState<TurnDiff | null>(null);
  const [loadingTurnDiff, setLoadingTurnDiff] = useState(false);
  // Measured over the whole session, so unlike the doctor report this survives
  // moving the selected turn and is cleared only when the session changes.
  const [residual, setResidual] = useState<ResidualReport | null>(null);
  const [loadingResidual, setLoadingResidual] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showRoots, setShowRoots] = useState(false);
  // Which session the turn-comparison panel's right side names, when it is
  // not the currently open session's own slider turn. `crossTurnInput` stays
  // a string rather than a number so the field can sit empty mid-edit
  // without snapping to 0, which would silently ask to compare against turn
  // zero -- a request no session can answer.
  const [crossSession, setCrossSession] = useState<{ agent: Agent; id: string } | null>(null);
  const [crossTurnInput, setCrossTurnInput] = useState("");
  const [archive, setArchive] = useState<ArchiveHolding | null>(null);
  const [loadingArchive, setLoadingArchive] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const [archiveRaw, setArchiveRaw] = useState(false);
  const [verifications, setVerifications] = useState<
    Record<string, ArchiveVerification | "loading" | Error>
  >({});
  const [exporting, setExporting] = useState(false);
  const [exportOutcome, setExportOutcome] = useState<ExportOutcome | null>(null);
  const [exportError, setExportError] = useState<string | null>(null);
  // Redaction is the default here even though `ct export`'s is not; see
  // `ExportControl` for why the destination changes the answer.
  const [exportKeepSecrets, setExportKeepSecrets] = useState(false);
  const [instructionFiles, setInstructionFiles] = useState<InstructionFileReport | null>(null);
  const [instructionFilesLoading, setInstructionFilesLoading] = useState(false);
  const [ghost, setGhost] = useState<TemporalGhost | null>(null);
  const [ghostLoading, setGhostLoading] = useState(false);
  const [cost, setCost] = useState<CostReport | null>(null);
  const [costLoading, setCostLoading] = useState(false);
  const sessionRequest = useRef(0);
  const turnRequest = useRef(0);
  const doctorRequest = useRef(0);
  const lifecycleRequest = useRef(0);
  const compactionRequest = useRef(0);
  const turnDiffRequest = useRef(0);
  const residualRequest = useRef(0);
  const catalogRequest = useRef(0);
  const archiveRequest = useRef(0);
  const evidenceRequest = useRef(0);

  const refreshSessions = useCallback(
    async (forceRefresh = false) => {
      const request = ++catalogRequest.current;
      setLoadingSessions(true);
      setLoadingMore(false);
      setError(null);
      try {
        const page = await api.searchSessions(
          agentFilter === "all" ? undefined : agentFilter,
          debouncedQuery,
          0,
          200,
          forceRefresh,
        );
        if (request !== catalogRequest.current) return;
        setSessions(page.sessions);
        setSessionTotal(page.total);
        setHasMoreSessions(page.hasMore);
        setSelected((current) =>
          current &&
          page.sessions.some(
            (session) => session.agent === current.agent && session.id === current.id,
          )
            ? current
            : page.sessions[0]
              ? { agent: page.sessions[0].agent, id: page.sessions[0].id }
              : null,
        );
      } catch (loadError) {
        if (request === catalogRequest.current) setError(errorMessage(loadError));
      } finally {
        if (request === catalogRequest.current) setLoadingSessions(false);
      }
    },
    [agentFilter, debouncedQuery],
  );

  const loadMoreSessions = useCallback(async () => {
    if (loadingMore || !hasMoreSessions) return;
    const request = ++catalogRequest.current;
    setLoadingMore(true);
    setError(null);
    try {
      const page = await api.searchSessions(
        agentFilter === "all" ? undefined : agentFilter,
        debouncedQuery,
        sessions.length,
        200,
        false,
      );
      if (request !== catalogRequest.current) return;
      setSessions((current) => {
        const existing = new Set(current.map((session) => `${session.agent}:${session.id}`));
        return [
          ...current,
          ...page.sessions.filter(
            (session) => !existing.has(`${session.agent}:${session.id}`),
          ),
        ];
      });
      setSessionTotal(page.total);
      setHasMoreSessions(page.hasMore);
    } catch (loadError) {
      if (request === catalogRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === catalogRequest.current) setLoadingMore(false);
    }
  }, [
    agentFilter,
    debouncedQuery,
    hasMoreSessions,
    loadingMore,
    sessions.length,
  ]);

  useEffect(() => {
    api.getStartup().then(setStartup).catch((loadError) => setError(errorMessage(loadError)));
  }, []);

  useEffect(() => {
    const timer = window.setTimeout(() => setDebouncedQuery(query.trim()), 200);
    return () => window.clearTimeout(timer);
  }, [query]);

  useEffect(() => {
    refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    if (!selected) {
      sessionRequest.current += 1;
      turnRequest.current += 1;
      doctorRequest.current += 1;
      lifecycleRequest.current += 1;
      compactionRequest.current += 1;
      residualRequest.current += 1;
      setDetail(null);
      setDetailFor(null);
      setContext(null);
      setDoctor(null);
      setLifecycle(null);
      setLifecycleItem(null);
      setCompactionDiff(null);
      setCompactionLineNo(null);
      setResidual(null);
      setLoadingDetail(false);
      setLoadingContext(false);
      setLoadingDoctor(false);
      setLoadingLifecycle(false);
      setLoadingCompaction(false);
      setLoadingResidual(false);
      setPinnedTurn(null);
      setCrossSession(null);
      setCrossTurnInput("");
      setExportOutcome(null);
      setExportError(null);
      setExporting(false);
      setInstructionFiles(null);
      setGhost(null);
      setInstructionFilesLoading(false);
      setGhostLoading(false);
      setCost(null);
      setCostLoading(false);
      return;
    }
    const request = ++sessionRequest.current;
    turnRequest.current += 1;
    doctorRequest.current += 1;
    lifecycleRequest.current += 1;
    compactionRequest.current += 1;
    residualRequest.current += 1;
    setDetail(null);
    setDetailFor(null);
    setContext(null);
    setDoctor(null);
    setLifecycle(null);
    setLifecycleItem(null);
    setCompactionDiff(null);
    setCompactionLineNo(null);
    setResidual(null);
    setInstructionFiles(null);
    setGhost(null);
    setCost(null);
    setLoadingDetail(true);
    setLoadingContext(false);
    setLoadingDoctor(false);
    setLoadingLifecycle(false);
    setLoadingCompaction(false);
    setLoadingResidual(false);
    setInstructionFilesLoading(false);
    setGhostLoading(false);
    setCostLoading(false);
    // The baseline, the cross-session pick and the last export result all
    // name a specific session; carrying any of them into a newly selected one
    // would present a stale answer as though it were about the session now on
    // screen. The pin is the sharpest case: it is a bare turn number, so it
    // would silently rebase onto the new session -- at a turn the user picked
    // for a different one, and which this session may not even have.
    setPinnedTurn(null);
    setCrossSession(null);
    setCrossTurnInput("");
    setExportOutcome(null);
    setExportError(null);
    setExporting(false);
    setError(null);
    Promise.all([
      api.inspectSession(selected.agent, selected.id),
      api.getContext(selected.agent, selected.id),
    ])
      .then(([nextDetail, nextContext]) => {
        if (request !== sessionRequest.current) return;
        setDetail(nextDetail);
        setDetailFor(selected);
        setContext(nextContext);
      })
      .catch((loadError) => {
        if (request === sessionRequest.current) setError(errorMessage(loadError));
      })
      .finally(() => {
        if (request === sessionRequest.current) setLoadingDetail(false);
      });
  }, [selected]);

  const selectTurn = useCallback(
    async (turn: number) => {
      if (!selected || turn === context?.turn) return;
      const request = ++turnRequest.current;
      doctorRequest.current += 1;
      lifecycleRequest.current += 1;
      const session = selected;
      setContext(null);
      setDoctor(null);
      setLifecycle(null);
      setLifecycleItem(null);
      setLoadingDoctor(false);
      setLoadingLifecycle(false);
      setLoadingContext(true);
      setError(null);
      try {
        const nextContext = await api.getContext(session.agent, session.id, turn);
        if (request === turnRequest.current && sameSession(session, selected)) {
          setContext(nextContext);
        }
      } catch (loadError) {
        if (request === turnRequest.current && sameSession(session, selected)) {
          setError(errorMessage(loadError));
        }
      } finally {
        if (request === turnRequest.current && sameSession(session, selected)) {
          setLoadingContext(false);
        }
      }
    },
    [context?.turn, selected],
  );

  const runDoctor = useCallback(async () => {
    if (!selected || !context) return;
    const request = ++doctorRequest.current;
    const session = selected;
    const turn = context.turn;
    setLoadingDoctor(true);
    setError(null);
    try {
      const report = await api.runDoctor(session.agent, session.id, turn);
      if (
        request === doctorRequest.current &&
        sameSession(session, selected) &&
        turn === context.turn
      ) {
        setDoctor(report);
      }
    } catch (loadError) {
      if (request === doctorRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === doctorRequest.current) setLoadingDoctor(false);
    }
  }, [context, selected]);

  /**
   * Measure the whole session's unlogged remainder.
   *
   * Depends on `selected` alone, not on the selected turn: the measurement is a
   * property of the session, and re-running it every time the turn slider moves
   * would pay for a full-session reconstruction to produce the same answer.
   */
  const runResidual = useCallback(async () => {
    if (!selected) return;
    const request = ++residualRequest.current;
    const session = selected;
    setLoadingResidual(true);
    setError(null);
    try {
      const report = await api.getResidual(session.agent, session.id);
      if (request === residualRequest.current && sameSession(session, selected)) {
        setResidual(report);
      }
    } catch (loadError) {
      if (request === residualRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === residualRequest.current) setLoadingResidual(false);
    }
  }, [selected]);

  const runInstructionFiles = useCallback(async () => {
    if (!selected) return;
    const request = ++evidenceRequest.current;
    const session = selected;
    setInstructionFilesLoading(true);
    setError(null);
    try {
      const report = await api.getInstructionFiles(session.agent, session.id);
      if (request === evidenceRequest.current && sameSession(session, selected)) {
        setInstructionFiles(report);
      }
    } catch (loadError) {
      if (request === evidenceRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === evidenceRequest.current) setInstructionFilesLoading(false);
    }
  }, [selected]);

  const runGhost = useCallback(async () => {
    if (!selected || !context || pinnedTurn == null || pinnedTurn === context.turn) return;
    const request = ++evidenceRequest.current;
    const session = selected;
    setGhostLoading(true);
    setError(null);
    try {
      const report = await api.getTemporalGhost(
        session.agent,
        session.id,
        pinnedTurn,
        context.turn,
      );
      if (request === evidenceRequest.current && sameSession(session, selected)) {
        setGhost(report);
      }
    } catch (loadError) {
      if (request === evidenceRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === evidenceRequest.current) setGhostLoading(false);
    }
  }, [context, pinnedTurn, selected]);

  const runCost = useCallback(
    async (pricingPath: string | null, forecastTurns: number | null) => {
      if (!selected) return;
      const request = ++evidenceRequest.current;
      const session = selected;
      setCostLoading(true);
      setError(null);
      try {
        const report = await api.getCost(session.agent, session.id, pricingPath, forecastTurns);
        if (request === evidenceRequest.current && sameSession(session, selected)) {
          setCost(report);
        }
      } catch (loadError) {
        if (request === evidenceRequest.current) setError(errorMessage(loadError));
      } finally {
        if (request === evidenceRequest.current) setCostLoading(false);
      }
    },
    [selected],
  );

  const inspectContributor = useCallback(
    async (item: string) => {
      if (!selected) return;
      const request = ++lifecycleRequest.current;
      const session = selected;
      setLifecycleItem(item);
      setLifecycle(null);
      setLoadingLifecycle(true);
      setError(null);
      try {
        const report = await api.getLifecycle(session.agent, session.id, item);
        if (request === lifecycleRequest.current && sameSession(session, selected)) {
          setLifecycle(report);
        }
      } catch (loadError) {
        if (request === lifecycleRequest.current) setError(errorMessage(loadError));
      } finally {
        if (request === lifecycleRequest.current) setLoadingLifecycle(false);
      }
    },
    [selected],
  );

  const closeLifecycle = useCallback(() => {
    lifecycleRequest.current += 1;
    setLifecycle(null);
    setLifecycleItem(null);
    setLoadingLifecycle(false);
  }, []);

  const inspectCompaction = useCallback(
    async (lineNo: number) => {
      if (!selected) return;
      const request = ++compactionRequest.current;
      const session = selected;
      setCompactionLineNo(lineNo);
      setCompactionDiff(null);
      setLoadingCompaction(true);
      setError(null);
      try {
        const diff = await api.getCompactionDiff(session.agent, session.id, lineNo);
        if (request === compactionRequest.current && sameSession(session, selected)) {
          setCompactionDiff(diff);
        }
      } catch (loadError) {
        if (request === compactionRequest.current) setError(errorMessage(loadError));
      } finally {
        if (request === compactionRequest.current) setLoadingCompaction(false);
      }
    },
    [selected],
  );

  const closeCompaction = useCallback(() => {
    compactionRequest.current += 1;
    setCompactionDiff(null);
    setCompactionLineNo(null);
    setLoadingCompaction(false);
  }, []);

  // Recomputed whenever either end moves, so scrubbing the slider with a turn
  // pinned reads as a live comparison rather than as a stale one the user has
  // to remember to refresh.
  const comparisonTurn = context?.turn ?? null;

  // The baseline is always a turn of the currently open session -- only the
  // right side can name a different one.
  const leftTarget: TurnTarget | null =
    selected && pinnedTurn != null ? { agent: selected.agent, id: selected.id, turn: pinnedTurn } : null;

  const crossTurnParsed = crossTurnInput.trim() === "" ? null : Number(crossTurnInput);
  const crossTurnValid =
    crossTurnParsed != null && Number.isInteger(crossTurnParsed) && crossTurnParsed >= 1;
  const rightTarget: TurnTarget | null = crossSession
    ? crossTurnValid
      ? { agent: crossSession.agent, id: crossSession.id, turn: crossTurnParsed as number }
      : null
    : selected && comparisonTurn != null
      ? { agent: selected.agent, id: selected.id, turn: comparisonTurn }
      : null;

  useEffect(() => {
    if (!leftTarget || !rightTarget || sameTarget(leftTarget, rightTarget)) {
      // Comparing a turn with itself is a valid request with a useless answer
      // (or, with nothing picked yet, no request at all); saying so beats
      // rendering a table of zeroes. Identity, not just the turn number,
      // decides this now: turn 5 of one session against turn 5 of another is
      // a real comparison, not a self-compare.
      setTurnDiff(null);
      return;
    }
    const request = ++turnDiffRequest.current;
    let cancelled = false;
    setLoadingTurnDiff(true);
    api
      .getTurnDiff(leftTarget, rightTarget)
      .then((diff) => {
        if (!cancelled && request === turnDiffRequest.current) setTurnDiff(diff);
      })
      .catch((loadError) => {
        if (!cancelled && request === turnDiffRequest.current) setError(errorMessage(loadError));
      })
      .finally(() => {
        if (!cancelled && request === turnDiffRequest.current) setLoadingTurnDiff(false);
      });
    return () => {
      cancelled = true;
    };
    // Depends on the scalar fields of `leftTarget`/`rightTarget` rather than
    // the objects themselves: both are plain literals rebuilt fresh every
    // render, so depending on their identity would refire this effect on
    // every render that touches unrelated state, not just when the turns or
    // sessions being compared actually change.
  }, [leftTarget?.agent, leftTarget?.id, leftTarget?.turn, rightTarget?.agent, rightTarget?.id, rightTarget?.turn]);

  const togglePin = useCallback(() => {
    setPinnedTurn((current) => (current == null ? (comparisonTurn ?? null) : null));
  }, [comparisonTurn]);

  const loadArchive = useCallback(async () => {
    const request = ++archiveRequest.current;
    setLoadingArchive(true);
    try {
      const holding = await api.archivedSessions();
      if (request === archiveRequest.current) setArchive(holding);
    } catch (loadError) {
      if (request === archiveRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === archiveRequest.current) setLoadingArchive(false);
    }
  }, []);

  useEffect(() => {
    loadArchive();
  }, [loadArchive]);

  const archiveSelected = useCallback(async () => {
    if (!selected) return;
    setArchiving(true);
    setError(null);
    try {
      await api.archiveSession(selected.agent, selected.id, archiveRaw);
      // Re-read rather than splice the new entry in locally: the holding's
      // `root` and ordering are the backend's to state, not this component's
      // to reconstruct from one call's answer.
      await loadArchive();
    } catch (archiveErrorValue) {
      setError(errorMessage(archiveErrorValue));
    } finally {
      setArchiving(false);
    }
  }, [selected, archiveRaw, loadArchive]);

  const verifyEntry = useCallback(async (agent: Agent, id: string) => {
    const key = `${agent}:${id}`;
    setVerifications((current) => ({ ...current, [key]: "loading" }));
    try {
      const result = await api.verifyArchived(agent, id);
      setVerifications((current) => ({ ...current, [key]: result }));
    } catch (verifyErrorValue) {
      setVerifications((current) => ({
        ...current,
        [key]: verifyErrorValue instanceof Error ? verifyErrorValue : new Error(errorMessage(verifyErrorValue)),
      }));
    }
  }, []);

  const exportSelected = useCallback(async () => {
    if (!selected) return;
    setExporting(true);
    setExportError(null);
    try {
      const outcome = await api.exportSession(selected.agent, selected.id, !exportKeepSecrets);
      setExportOutcome(outcome);
    } catch (exportErrorValue) {
      setExportError(errorMessage(exportErrorValue));
      setExportOutcome(null);
    } finally {
      setExporting(false);
    }
  }, [selected, exportKeepSecrets]);

  const visibleDetail = sameSession(detailFor, selected) ? detail : null;
  const scrollToSection = useCallback((id: string) => {
    document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });
  }, []);

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <header className="brand">
          <span className="brand-glyph">
            <i />
            <i />
            <i />
          </span>
          <div>
            <strong>ContextTrace</strong>
            <span>See what fills your AI context</span>
          </div>
        </header>

        <div className="search-box">
          <span aria-hidden="true">⌕</span>
          <input
            type="search"
            placeholder="Find a project or session…"
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            aria-label="Search sessions"
          />
          {query && (
            <button onClick={() => setQuery("")} aria-label="Clear search">
              ×
            </button>
          )}
        </div>

        <div className="filter-row" role="group" aria-label="Filter sessions by agent">
          {(["all", "codex", "claude-code"] as AgentFilter[]).map((agent) => (
            <button
              key={agent}
              className={agentFilter === agent ? "active" : ""}
              onClick={() => setAgentFilter(agent)}
              aria-pressed={agentFilter === agent}
            >
              {agent === "all" ? "All" : agent === "codex" ? "Codex" : "Claude"}
            </button>
          ))}
          <button
            className="refresh"
            onClick={() => refreshSessions(true)}
            aria-label="Refresh sessions"
            disabled={loadingSessions}
          >
            ↻
          </button>
        </div>

        <nav className="primary-nav" aria-label="Workspace sections">
          <span className="nav-label">Workspace</span>
          {[
            ["overview", "Overview", "Start with the session story"],
            ["compare", "Compare", "See what changed between turns"],
            ["insights", "Context insights", "Find waste and hidden changes"],
            ["archive", "Save & export", "Keep or share the evidence"],
          ].map(([id, label, detail]) => (
            <button key={id} type="button" onClick={() => scrollToSection(id)}>
              <span className="nav-index">{String(["overview", "compare", "insights", "archive"].indexOf(id) + 1).padStart(2, "0")}</span>
              <span>
                <strong>{label}</strong>
                <small>{detail}</small>
              </span>
            </button>
          ))}
        </nav>

        <div className="session-list-heading" aria-live="polite" aria-atomic="true">
          <span>{demoData ? "Demonstration sessions" : "Your sessions"}</span>
          <span>
            {sessions.length === sessionTotal
              ? sessionTotal
              : `${sessions.length} / ${sessionTotal}`}
          </span>
        </div>

        <nav className="session-list" aria-label="Sessions" aria-busy={loadingSessions}>
          {loadingSessions ? (
            <Spinner label="Discovering local sessions…" />
          ) : sessions.length ? (
            <>
              {sessions.map((session) => (
                <SessionListItem
                  key={`${session.agent}-${session.id}`}
                  session={session}
                  selected={selected?.agent === session.agent && selected?.id === session.id}
                  onSelect={() => setSelected({ agent: session.agent, id: session.id })}
                />
              ))}
              {hasMoreSessions && (
                <button
                  className="load-more"
                  onClick={loadMoreSessions}
                  disabled={loadingMore}
                >
                  {loadingMore ? "Loading…" : `Load more (${sessionTotal - sessions.length})`}
                </button>
              )}
            </>
          ) : (
            <div className="empty-list" role="status">
              <strong>No matching sessions</strong>
              <span>Try another project name or agent.</span>
            </div>
          )}
        </nav>

        <footer className="sidebar-footer">
          <button
            onClick={() => setShowRoots((value) => !value)}
            aria-expanded={showRoots}
            aria-controls="local-log-roots"
          >
            <span className={demoData ? "shield demo-shield" : "shield"}>
              {demoData ? "!" : "✓"}
            </span>
            <span>
              <strong>{demoData ? "Demonstration data" : "Private by design"}</strong>
              <small>
                {demoData
                  ? "Fabricated fixtures. No local logs are being read."
                  : "Reads local logs. No network."}
              </small>
            </span>
            <span>{showRoots ? "⌃" : "⌄"}</span>
          </button>
          {showRoots && startup && (
            <div className="roots" id="local-log-roots">
              <div className="roots-group">
                <span className="roots-group-label">Reads</span>
                {startup.roots.map((root) => (
                  <div key={root.agent}>
                    <strong>{root.agent}</strong>
                    {root.paths.map((path) => (
                      <code key={path}>{path}</code>
                    ))}
                  </div>
                ))}
              </div>
              {/* The one directory this tool writes to, in the same
                  disclosure as the ones it only reads -- distinguished by
                  its own label rather than folded into the list above,
                  where it would read as another source being observed. */}
              <div className="roots-group">
                <span className="roots-group-label">Writes</span>
                <div>
                  <strong>archive</strong>
                  <code>{startup.archiveRoot}</code>
                </div>
              </div>
            </div>
          )}
        </footer>
      </aside>

      <div
        className={demoData ? "main-area demo-mode" : "main-area"}
        aria-busy={loadingDetail}
      >
        {demoData && (
          <div className="demo-banner" role="status">
            <span aria-hidden="true">◆</span>
            <p>
              <strong>Demonstration data.</strong>
              The desktop bridge is not available, so every session, token count and
              finding on this screen is fabricated. Run the ContextTrace desktop app
              to read your own local logs.
            </p>
          </div>
        )}
        {error && (
          <div className="error-banner" role="alert" aria-atomic="true">
            <span aria-hidden="true">!</span>
            <p><strong>ContextTrace couldn’t complete that view.</strong>{error}</p>
            <button onClick={() => setError(null)}>Dismiss</button>
          </div>
        )}
        {startup?.warnings.map((warning) => (
          <div className="warning-banner" key={warning} role="status">{warning}</div>
        ))}
        {loadingDetail && !visibleDetail ? (
          <div className="workspace-centered">
            <Spinner label="Reading session…" />
          </div>
        ) : visibleDetail ? (
          <SessionWorkspace
            detail={visibleDetail}
            context={context}
            contextLoading={loadingContext}
            doctor={doctor}
            doctorLoading={loadingDoctor}
            lifecycle={lifecycle}
            lifecycleLoading={loadingLifecycle}
            lifecycleItem={lifecycleItem}
            compactionDiff={compactionDiff}
            compactionLoading={loadingCompaction}
            compactionLineNo={compactionLineNo}
            turnDiff={turnDiff}
            turnDiffLoading={loadingTurnDiff}
            pinnedTurn={pinnedTurn}
            onTogglePin={togglePin}
            sessions={sessions}
            leftTarget={leftTarget}
            rightTarget={rightTarget}
            crossSession={crossSession}
            crossTurnInput={crossTurnInput}
            onSetCrossSession={setCrossSession}
            onSetCrossTurnInput={setCrossTurnInput}
            residual={residual}
            residualLoading={loadingResidual}
            onRunResidual={runResidual}
            demoData={demoData}
            archive={archive}
            archiveLoading={loadingArchive}
            archiving={archiving}
            archiveRaw={archiveRaw}
            onToggleArchiveRaw={setArchiveRaw}
            onArchiveSelected={archiveSelected}
            verifications={verifications}
            onVerifyEntry={verifyEntry}
            exporting={exporting}
            exportOutcome={exportOutcome}
            exportError={exportError}
            exportKeepSecrets={exportKeepSecrets}
            onToggleExportKeep={setExportKeepSecrets}
            onExportSelected={exportSelected}
            onContributor={inspectContributor}
            onCloseLifecycle={closeLifecycle}
            onRunDoctor={runDoctor}
            onTurn={selectTurn}
            onCompaction={inspectCompaction}
            onCloseCompaction={closeCompaction}
            instructionFiles={instructionFiles}
            instructionFilesLoading={instructionFilesLoading}
            onRunInstructionFiles={runInstructionFiles}
            ghost={ghost}
            ghostLoading={ghostLoading}
            onRunGhost={runGhost}
            cost={cost}
            costLoading={costLoading}
            onRunCost={runCost}
          />
        ) : (
          <div className="workspace-centered empty-workspace">
            <span className="empty-glyph">◎</span>
            <h1>Select a session</h1>
            <p>Choose a Codex or Claude Code run to see where its context went.</p>
          </div>
        )}
      </div>
    </div>
  );
}
