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
  Comparability,
  CompactionDiff,
  CompactionDiffItem,
  CompactionDiffUnavailableReason,
  CompactionItemDisposition,
  ContextDetail,
  DoctorReport,
  GrowthPoint,
  LifecycleReport,
  SessionDetail,
  SessionSummary,
  StartupSummary,
  TurnDiff,
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
 */
function ComparabilityNote({ comparability }: { comparability: Comparability }) {
  if (comparability.kind === "identical") {
    return (
      <p className="diff-instrument">
        <strong>{comparability.estimator}</strong> sized both turns — one session, one
        instrument, so every delta below is content.
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

function TurnComparison({
  diff,
  loading,
  pinnedTurn,
  comparisonTurn,
}: {
  diff: TurnDiff | null;
  loading: boolean;
  pinnedTurn: number | null;
  comparisonTurn: number | null;
}) {
  if (pinnedTurn == null) return null;
  if (loading && !diff) return <Spinner label="Comparing turns…" />;
  if (pinnedTurn === comparisonTurn) {
    return (
      <section className="panel diff-panel" aria-labelledby="diff-heading">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Turn comparison</span>
            <h2 id="diff-heading">Baseline pinned at turn {pinnedTurn}</h2>
          </div>
        </div>
        <p className="diff-empty">
          Scrub the slider or pick a point on the chart to choose a turn to compare this
          one against.
        </p>
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
            Turn {diff.left.turn} → turn {diff.right.turn}
          </h2>
        </div>
        {diff.totalsAreObserved && (
          <span className="panel-total">
            {signed(diff.promptDelta)} tokens reported
          </span>
        )}
      </div>

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

function ContextComposition({ context }: { context: ContextDetail }) {
  const categories = Array.isArray(context.categories) ? context.categories : [];
  return (
    <section className="panel composition-panel" aria-labelledby="composition-heading">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Composition</span>
          <h2 id="composition-heading">What filled the prompt</h2>
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
          <h2 id="contributors-heading">Where to look first</h2>
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
          <h2 id="doctor-heading">Find avoidable context and potential credential exposure</h2>
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
  demoData,
  onContributor,
  onCloseLifecycle,
  onRunDoctor,
  onTurn,
  onCompaction,
  onCloseCompaction,
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
  demoData: boolean;
  onContributor: (item: string) => void;
  onCloseLifecycle: () => void;
  onRunDoctor: () => void;
  onTurn: (turn: number) => void;
  onCompaction: (lineNo: number) => void;
  onCloseCompaction: () => void;
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
          label="Peak prompt"
          value={formatTokens(detail.peakPromptTokens)}
          note={detail.peakTurn ? `at turn ${detail.peakTurn}` : "No usage recorded"}
          accent
        />
        <Metric
          label="Window used"
          value={formatPercent(peakUtilisation)}
          note={
            detail.contextWindow
              ? `${formatTokens(detail.contextWindow)} token window`
              : "Window not reported"
          }
        />
        <Metric
          label="Conversation"
          value={`${detail.turnCount}`}
          note={`${detail.eventCount.toLocaleString()} events`}
        />
        <Metric
          label="Parse fidelity"
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
            <span className="eyebrow">Prompt growth</span>
            <h2 id="timeline-heading">Session timeline</h2>
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

      <TurnComparison
        diff={turnDiff}
        loading={turnDiffLoading}
        pinnedTurn={pinnedTurn}
        comparisonTurn={context?.turn ?? null}
      />

      <CompactionAutopsy
        diff={compactionDiff}
        loading={compactionLoading}
        onClose={onCloseCompaction}
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
  const [error, setError] = useState<string | null>(null);
  const [showRoots, setShowRoots] = useState(false);
  const sessionRequest = useRef(0);
  const turnRequest = useRef(0);
  const doctorRequest = useRef(0);
  const lifecycleRequest = useRef(0);
  const compactionRequest = useRef(0);
  const turnDiffRequest = useRef(0);
  const catalogRequest = useRef(0);

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
      setDetail(null);
      setDetailFor(null);
      setContext(null);
      setDoctor(null);
      setLifecycle(null);
      setLifecycleItem(null);
      setCompactionDiff(null);
      setCompactionLineNo(null);
      setLoadingDetail(false);
      setLoadingContext(false);
      setLoadingDoctor(false);
      setLoadingLifecycle(false);
      setLoadingCompaction(false);
      return;
    }
    const request = ++sessionRequest.current;
    turnRequest.current += 1;
    doctorRequest.current += 1;
    lifecycleRequest.current += 1;
    compactionRequest.current += 1;
    setDetail(null);
    setDetailFor(null);
    setContext(null);
    setDoctor(null);
    setLifecycle(null);
    setLifecycleItem(null);
    setCompactionDiff(null);
    setCompactionLineNo(null);
    setLoadingDetail(true);
    setLoadingContext(false);
    setLoadingDoctor(false);
    setLoadingLifecycle(false);
    setLoadingCompaction(false);
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
  useEffect(() => {
    if (!selected || pinnedTurn == null || comparisonTurn == null) {
      setTurnDiff(null);
      return;
    }
    if (pinnedTurn === comparisonTurn) {
      // Comparing a turn with itself is a valid request with a useless answer;
      // saying so beats rendering a table of zeroes.
      setTurnDiff(null);
      return;
    }
    const request = ++turnDiffRequest.current;
    const session = selected;
    let cancelled = false;
    setLoadingTurnDiff(true);
    api
      .getTurnDiff(session.agent, session.id, pinnedTurn, comparisonTurn)
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
  }, [selected, pinnedTurn, comparisonTurn]);

  const togglePin = useCallback(() => {
    setPinnedTurn((current) => (current == null ? (comparisonTurn ?? null) : null));
  }, [comparisonTurn]);

  const visibleDetail = sameSession(detailFor, selected) ? detail : null;

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
            <span>Local context inspector</span>
          </div>
        </header>

        <div className="search-box">
          <span aria-hidden="true">⌕</span>
          <input
            type="search"
            placeholder="Search sessions or projects"
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

        <div className="session-list-heading" aria-live="polite" aria-atomic="true">
          <span>{demoData ? "Demonstration sessions" : "Recent sessions"}</span>
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
              {startup.roots.map((root) => (
                <div key={root.agent}>
                  <strong>{root.agent}</strong>
                  {root.paths.map((path) => (
                    <code key={path}>{path}</code>
                  ))}
                </div>
              ))}
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
            demoData={demoData}
            onContributor={inspectContributor}
            onCloseLifecycle={closeLifecycle}
            onRunDoctor={runDoctor}
            onTurn={selectTurn}
            onCompaction={inspectCompaction}
            onCloseCompaction={closeCompaction}
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
