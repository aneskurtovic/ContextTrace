import { useCallback, useEffect, useMemo, useState } from "react";
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
  GrowthPoint,
  SessionDetail,
  SessionSummary,
  StartupSummary,
} from "./types";

type AgentFilter = "all" | Agent;

function AgentMark({ agent }: { agent: Agent }) {
  return (
    <span className={`agent-mark ${agent}`} aria-label={agent}>
      {agent === "codex" ? "CX" : "CC"}
    </span>
  );
}

function Spinner({ label }: { label: string }) {
  return (
    <div className="loading" role="status">
      <span className="spinner" />
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
      aria-pressed={selected}
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
    return <div className="chart-empty">Not enough measured turns to chart.</div>;
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
          >
            <title>
              Turn {point.turn}: {point.promptTokens.toLocaleString()} tokens
            </title>
          </circle>
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
  return (
    <section className="panel composition-panel">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Composition</span>
          <h2>What filled the prompt</h2>
        </div>
        <span className="panel-total">{context.totalTokens.toLocaleString()} tokens</span>
      </div>
      <div className="composition-list">
        {context.categories.map((category) => (
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
        ))}
      </div>
      {context.residualIsMeaningful && context.residualTokens > 0 && (
        <p className="callout">
          <span>?</span>
          {context.residualTokens.toLocaleString()} tokens are present in the reported total but
          not attributable from the session log—usually hidden system instructions, tool schemas,
          or request framing.
        </p>
      )}
    </section>
  );
}

function Contributors({ context }: { context: ContextDetail }) {
  return (
    <section className="panel contributors-panel">
      <div className="panel-heading">
        <div>
          <span className="eyebrow">Largest contributors</span>
          <h2>Where to look first</h2>
        </div>
        <span className="count-pill">{context.contributors.length} shown</span>
      </div>
      <div className="contributor-list">
        {context.contributors.map((item, index) => (
          <div className="contributor-row" key={item.id}>
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
          </div>
        ))}
      </div>
    </section>
  );
}

function SessionWorkspace({
  detail,
  context,
  contextLoading,
  onTurn,
}: {
  detail: SessionDetail;
  context: ContextDetail | null;
  contextLoading: boolean;
  onTurn: (turn: number) => void;
}) {
  const measuredTurns = detail.growth.filter((point) => point.promptTokens != null);
  const selectedIndex = Math.max(
    0,
    measuredTurns.findIndex((point) => point.turn === context?.turn),
  );
  const peakUtilisation =
    detail.peakPromptTokens && detail.contextWindow
      ? detail.peakPromptTokens / detail.contextWindow
      : null;

  return (
    <main className="workspace">
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
        <div className="privacy-badge">
          <span className="privacy-dot" />
          Local only
        </div>
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

      <section className="panel timeline-panel">
        <div className="panel-heading">
          <div>
            <span className="eyebrow">Prompt growth</span>
            <h2>Session timeline</h2>
          </div>
          <div className="legend">
            <span><i className="legend-growth" /> prompt size</span>
            <span><i className="legend-compaction" /> compaction</span>
          </div>
        </div>
        <GrowthChart
          points={detail.growth}
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
              {context ? `${context.totalTokens.toLocaleString()} tokens` : "Loading context"}
            </span>
          </div>
        )}
      </section>

      {contextLoading && !context ? (
        <Spinner label="Reconstructing context…" />
      ) : context ? (
        <div className={contextLoading ? "context-grid refreshing" : "context-grid"}>
          <ContextComposition context={context} />
          <Contributors context={context} />
        </div>
      ) : (
        <div className="empty-inline">This session has no reconstructable prompt turn.</div>
      )}
    </main>
  );
}

export default function App() {
  const [startup, setStartup] = useState<StartupSummary | null>(null);
  const [sessions, setSessions] = useState<SessionSummary[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [detail, setDetail] = useState<SessionDetail | null>(null);
  const [context, setContext] = useState<ContextDetail | null>(null);
  const [agentFilter, setAgentFilter] = useState<AgentFilter>("all");
  const [query, setQuery] = useState("");
  const [loadingSessions, setLoadingSessions] = useState(true);
  const [loadingDetail, setLoadingDetail] = useState(false);
  const [loadingContext, setLoadingContext] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showRoots, setShowRoots] = useState(false);

  const refreshSessions = useCallback(async () => {
    setLoadingSessions(true);
    setError(null);
    try {
      const found = await api.listSessions(
        agentFilter === "all" ? undefined : agentFilter,
      );
      setSessions(found);
      setSelectedId((current) =>
        current && found.some((session) => session.id === current)
          ? current
          : found[0]?.id ?? null,
      );
    } catch (loadError) {
      setError(errorMessage(loadError));
    } finally {
      setLoadingSessions(false);
    }
  }, [agentFilter]);

  useEffect(() => {
    api.getStartup().then(setStartup).catch((loadError) => setError(errorMessage(loadError)));
  }, []);

  useEffect(() => {
    refreshSessions();
  }, [refreshSessions]);

  useEffect(() => {
    if (!selectedId) {
      setDetail(null);
      setContext(null);
      return;
    }
    let cancelled = false;
    setLoadingDetail(true);
    setError(null);
    Promise.all([api.inspectSession(selectedId), api.getContext(selectedId)])
      .then(([nextDetail, nextContext]) => {
        if (cancelled) return;
        setDetail(nextDetail);
        setContext(nextContext);
      })
      .catch((loadError) => {
        if (!cancelled) setError(errorMessage(loadError));
      })
      .finally(() => {
        if (!cancelled) setLoadingDetail(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedId]);

  const selectTurn = useCallback(
    async (turn: number) => {
      if (!selectedId || turn === context?.turn) return;
      setLoadingContext(true);
      try {
        setContext(await api.getContext(selectedId, turn));
      } catch (loadError) {
        setError(errorMessage(loadError));
      } finally {
        setLoadingContext(false);
      }
    },
    [context?.turn, selectedId],
  );

  const visibleSessions = useMemo(() => {
    const needle = query.trim().toLocaleLowerCase();
    if (!needle) return sessions;
    return sessions.filter((session) =>
      [session.project, session.id, session.path, session.agent]
        .filter(Boolean)
        .some((value) => value!.toLocaleLowerCase().includes(needle)),
    );
  }, [query, sessions]);

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

        <div className="filter-row">
          {(["all", "codex", "claude-code"] as AgentFilter[]).map((agent) => (
            <button
              key={agent}
              className={agentFilter === agent ? "active" : ""}
              onClick={() => setAgentFilter(agent)}
            >
              {agent === "all" ? "All" : agent === "codex" ? "Codex" : "Claude"}
            </button>
          ))}
          <button className="refresh" onClick={refreshSessions} aria-label="Refresh sessions">
            ↻
          </button>
        </div>

        <div className="session-list-heading">
          <span>Recent sessions</span>
          <span>{visibleSessions.length}</span>
        </div>

        <nav className="session-list" aria-label="Sessions">
          {loadingSessions ? (
            <Spinner label="Discovering local sessions…" />
          ) : visibleSessions.length ? (
            visibleSessions.map((session) => (
              <SessionListItem
                key={`${session.agent}-${session.id}`}
                session={session}
                selected={selectedId === session.id}
                onSelect={() => setSelectedId(session.id)}
              />
            ))
          ) : (
            <div className="empty-list">
              <strong>No matching sessions</strong>
              <span>Try another project name or agent.</span>
            </div>
          )}
        </nav>

        <footer className="sidebar-footer">
          <button onClick={() => setShowRoots((value) => !value)}>
            <span className="shield">✓</span>
            <span>
              <strong>Private by design</strong>
              <small>Reads local logs. No network.</small>
            </span>
            <span>{showRoots ? "⌃" : "⌄"}</span>
          </button>
          {showRoots && startup && (
            <div className="roots">
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

      <div className="main-area">
        {error && (
          <div className="error-banner" role="alert">
            <span>!</span>
            <p><strong>ContextTrace couldn’t complete that view.</strong>{error}</p>
            <button onClick={() => setError(null)}>Dismiss</button>
          </div>
        )}
        {startup?.warnings.map((warning) => (
          <div className="warning-banner" key={warning}>{warning}</div>
        ))}
        {loadingDetail && !detail ? (
          <div className="workspace-centered">
            <Spinner label="Reading session…" />
          </div>
        ) : detail ? (
          <SessionWorkspace
            detail={detail}
            context={context}
            contextLoading={loadingContext}
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
