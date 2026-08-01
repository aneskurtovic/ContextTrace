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
  ContextDetail,
  DoctorReport,
  GrowthPoint,
  LifecycleReport,
  SessionDetail,
  SessionSummary,
  StartupSummary,
} from "./types";

type AgentFilter = "all" | Agent;

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
  return (
    <button
      className={`session-row ${selected ? "selected" : ""}`}
      onClick={onSelect}
      aria-current={selected ? "true" : undefined}
      aria-label={`${agentLabel(session.agent)} session: ${projectName(session.project)}, ${shortId(session.id)}`}
    >
      <AgentMark agent={session.agent} />
      <span className="session-copy">
        <span className="session-title">{projectName(session.project)}</span>
        <span className="session-meta">
          {shortId(session.id)} · {formatBytes(session.sizeBytes)}
        </span>
      </span>
      <span className="session-activity">{formatActivity(session.lastActivity)}</span>
    </button>
  );
}

function agentLabel(agent: Agent) {
  return agent === "codex" ? "Codex" : "Claude Code";
}

function GrowthChart({
  points,
  selectedTurn,
  onTurn,
}: {
  points: GrowthPoint[];
  selectedTurn: number | null;
  onTurn: (turn: number) => void;
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
          .filter((point) => point.compaction)
          .map((point) => (
            <line
              key={`compaction-${point.turn}`}
              className="compaction-line"
              x1={x(point.turn)}
              x2={x(point.turn)}
              y1={10}
              y2={height - 10}
            />
          ))}
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
  demoData,
  onContributor,
  onCloseLifecycle,
  onRunDoctor,
  onTurn,
}: {
  detail: SessionDetail;
  context: ContextDetail | null;
  contextLoading: boolean;
  doctor: DoctorReport | null;
  doctorLoading: boolean;
  lifecycle: LifecycleReport | null;
  lifecycleLoading: boolean;
  lifecycleItem: string | null;
  demoData: boolean;
  onContributor: (item: string) => void;
  onCloseLifecycle: () => void;
  onRunDoctor: () => void;
  onTurn: (turn: number) => void;
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
          </div>
        )}
      </section>

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
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [detailForId, setDetailForId] = useState<string | null>(null);
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
  const [error, setError] = useState<string | null>(null);
  const [showRoots, setShowRoots] = useState(false);
  const sessionRequest = useRef(0);
  const turnRequest = useRef(0);
  const doctorRequest = useRef(0);
  const lifecycleRequest = useRef(0);
  const catalogRequest = useRef(0);

  const refreshSessions = useCallback(async () => {
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
      );
      if (request !== catalogRequest.current) return;
      setSessions(page.sessions);
      setSessionTotal(page.total);
      setHasMoreSessions(page.hasMore);
      setSelectedId((current) =>
        current && page.sessions.some((session) => session.id === current)
          ? current
          : page.sessions[0]?.id ?? null,
      );
    } catch (loadError) {
      if (request === catalogRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === catalogRequest.current) setLoadingSessions(false);
    }
  }, [agentFilter, debouncedQuery]);

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
    if (!selectedId) {
      sessionRequest.current += 1;
      turnRequest.current += 1;
      doctorRequest.current += 1;
      lifecycleRequest.current += 1;
      setDetail(null);
      setDetailForId(null);
      setContext(null);
      setDoctor(null);
      setLifecycle(null);
      setLifecycleItem(null);
      setLoadingDetail(false);
      setLoadingContext(false);
      setLoadingDoctor(false);
      setLoadingLifecycle(false);
      return;
    }
    const request = ++sessionRequest.current;
    turnRequest.current += 1;
    doctorRequest.current += 1;
    lifecycleRequest.current += 1;
    setDetail(null);
    setDetailForId(null);
    setContext(null);
    setDoctor(null);
    setLifecycle(null);
    setLifecycleItem(null);
    setLoadingDetail(true);
    setLoadingContext(false);
    setLoadingDoctor(false);
    setLoadingLifecycle(false);
    setError(null);
    Promise.all([api.inspectSession(selectedId), api.getContext(selectedId)])
      .then(([nextDetail, nextContext]) => {
        if (request !== sessionRequest.current) return;
        setDetail(nextDetail);
        setDetailForId(selectedId);
        setContext(nextContext);
      })
      .catch((loadError) => {
        if (request === sessionRequest.current) setError(errorMessage(loadError));
      })
      .finally(() => {
        if (request === sessionRequest.current) setLoadingDetail(false);
      });
  }, [selectedId]);

  const selectTurn = useCallback(
    async (turn: number) => {
      if (!selectedId || turn === context?.turn) return;
      const request = ++turnRequest.current;
      doctorRequest.current += 1;
      lifecycleRequest.current += 1;
      const session = selectedId;
      setContext(null);
      setDoctor(null);
      setLifecycle(null);
      setLifecycleItem(null);
      setLoadingDoctor(false);
      setLoadingLifecycle(false);
      setLoadingContext(true);
      setError(null);
      try {
        const nextContext = await api.getContext(session, turn);
        if (request === turnRequest.current && session === selectedId) {
          setContext(nextContext);
        }
      } catch (loadError) {
        if (request === turnRequest.current && session === selectedId) {
          setError(errorMessage(loadError));
        }
      } finally {
        if (request === turnRequest.current && session === selectedId) {
          setLoadingContext(false);
        }
      }
    },
    [context?.turn, selectedId],
  );

  const runDoctor = useCallback(async () => {
    if (!selectedId || !context) return;
    const request = ++doctorRequest.current;
    const session = selectedId;
    const turn = context.turn;
    setLoadingDoctor(true);
    setError(null);
    try {
      const report = await api.runDoctor(session, turn);
      if (
        request === doctorRequest.current &&
        session === selectedId &&
        turn === context.turn
      ) {
        setDoctor(report);
      }
    } catch (loadError) {
      if (request === doctorRequest.current) setError(errorMessage(loadError));
    } finally {
      if (request === doctorRequest.current) setLoadingDoctor(false);
    }
  }, [context, selectedId]);

  const inspectContributor = useCallback(
    async (item: string) => {
      if (!selectedId) return;
      const request = ++lifecycleRequest.current;
      const session = selectedId;
      setLifecycleItem(item);
      setLifecycle(null);
      setLoadingLifecycle(true);
      setError(null);
      try {
        const report = await api.getLifecycle(session, item);
        if (request === lifecycleRequest.current && session === selectedId) {
          setLifecycle(report);
        }
      } catch (loadError) {
        if (request === lifecycleRequest.current) setError(errorMessage(loadError));
      } finally {
        if (request === lifecycleRequest.current) setLoadingLifecycle(false);
      }
    },
    [selectedId],
  );

  const closeLifecycle = useCallback(() => {
    lifecycleRequest.current += 1;
    setLifecycle(null);
    setLifecycleItem(null);
    setLoadingLifecycle(false);
  }, []);

  const visibleDetail = detailForId === selectedId ? detail : null;

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
            onClick={refreshSessions}
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
                  selected={selectedId === session.id}
                  onSelect={() => setSelectedId(session.id)}
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
            demoData={demoData}
            onContributor={inspectContributor}
            onCloseLifecycle={closeLifecycle}
            onRunDoctor={runDoctor}
            onTurn={selectTurn}
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
