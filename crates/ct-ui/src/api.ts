import { invoke } from "@tauri-apps/api/core";
import type {
  Agent,
  ContextDetail,
  DoctorReport,
  LifecycleReport,
  SessionDetail,
  SessionPage,
  SessionSummary,
  StartupSummary,
} from "./types";
import {
  demoContext,
  demoDetail,
  demoDoctor,
  demoLifecycle,
  demoSessions,
  demoStartup,
} from "./demo";

const inTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Whether every figure on screen came from `demo.ts` instead of a local log.
 *
 * Without the desktop bridge each call below answers from fabricated fixtures,
 * which is useful for developing the interface and indefensible to show
 * unlabelled: this tool's whole claim is evidence over invention. The UI asks
 * this so it can say so wherever those numbers are rendered.
 */
export function isDemoData(): boolean {
  return !inTauri();
}

type UnknownRecord = Record<string, unknown>;

const agents = new Set(["codex", "claude-code"]);
const confidenceLevels = new Set(["observed", "derived", "estimated"]);

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null;
}

function isStringOrNull(value: unknown) {
  return typeof value === "string" || value === null;
}

function isNumberOrNull(value: unknown) {
  return typeof value === "number" || value === null;
}

function isSession(value: unknown): value is SessionSummary {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.agent === "string" &&
    agents.has(value.agent) &&
    typeof value.path === "string" &&
    typeof value.sizeBytes === "number" &&
    isStringOrNull(value.project) &&
    isStringOrNull(value.startedAt) &&
    isStringOrNull(value.lastActivity)
  );
}

function malformed(command: string): Error {
  return new Error(`ContextTrace received an invalid response from ${command}. Refresh and try again.`);
}

function asStartup(value: unknown): StartupSummary {
  if (
    !isRecord(value) ||
    !Array.isArray(value.roots) ||
    !value.roots.every(
      (root) =>
        isRecord(root) &&
        typeof root.agent === "string" &&
        agents.has(root.agent) &&
        Array.isArray(root.paths) &&
        root.paths.every((path) => typeof path === "string"),
    ) ||
    !Array.isArray(value.warnings) ||
    !value.warnings.every((warning) => typeof warning === "string")
  ) {
    throw malformed("startup");
  }
  return value as unknown as StartupSummary;
}

function asSessionPage(value: unknown): SessionPage {
  if (
    !isRecord(value) ||
    !Array.isArray(value.sessions) ||
    !value.sessions.every(isSession) ||
    typeof value.total !== "number" ||
    !Number.isInteger(value.total) ||
    value.total < 0 ||
    typeof value.offset !== "number" ||
    !Number.isInteger(value.offset) ||
    value.offset < 0 ||
    typeof value.hasMore !== "boolean"
  ) {
    throw malformed("session search");
  }
  return value as unknown as SessionPage;
}

function asDetail(value: unknown): SessionDetail {
  if (
    !isRecord(value) ||
    !isSession(value.session) ||
    !isStringOrNull(value.model) ||
    !isStringOrNull(value.agentVersion) ||
    !isStringOrNull(value.gitBranch) ||
    typeof value.turnCount !== "number" ||
    typeof value.eventCount !== "number" ||
    typeof value.totalOutputTokens !== "number" ||
    !isNumberOrNull(value.peakTurn) ||
    !isNumberOrNull(value.peakPromptTokens) ||
    !isNumberOrNull(value.contextWindow) ||
    typeof value.fidelity !== "number" ||
    typeof value.unrecognisedEvents !== "number" ||
    typeof value.unplacedCompactions !== "number" ||
    !Array.isArray(value.growth) ||
    !value.growth.every(
      (point) =>
        isRecord(point) &&
        typeof point.turn === "number" &&
        (typeof point.promptTokens === "number" || point.promptTokens === null) &&
        (point.compaction === null ||
          (isRecord(point.compaction) &&
            isNumberOrNull(point.compaction.turn) &&
            isNumberOrNull(point.compaction.reclaimed))),
    )
  ) {
    throw malformed("session inspection");
  }
  return value as unknown as SessionDetail;
}

function asContext(value: unknown): ContextDetail {
  if (
    !isRecord(value) ||
    typeof value.turn !== "number" ||
    !isStringOrNull(value.model) ||
    typeof value.totalTokens !== "number" ||
    typeof value.residualTokens !== "number" ||
    typeof value.residualIsMeaningful !== "boolean" ||
    !isNumberOrNull(value.contextWindow) ||
    !isNumberOrNull(value.utilisation) ||
    !isNumberOrNull(value.calibrationScale) ||
    !Array.isArray(value.categories) ||
    !value.categories.every(
      (category) =>
        isRecord(category) &&
        typeof category.category === "string" &&
        typeof category.label === "string" &&
        typeof category.tokens === "number" &&
        typeof category.share === "number" &&
        typeof category.itemCount === "number" &&
        typeof category.confidence === "string" &&
        confidenceLevels.has(category.confidence),
    ) ||
    !Array.isArray(value.contributors) ||
    !value.contributors.every(
      (contributor) =>
        isRecord(contributor) &&
        typeof contributor.id === "string" &&
        typeof contributor.label === "string" &&
        typeof contributor.category === "string" &&
        typeof contributor.source === "string" &&
        typeof contributor.tokens === "number" &&
        typeof contributor.share === "number" &&
        typeof contributor.confidence === "string" &&
        confidenceLevels.has(contributor.confidence),
    )
  ) {
    throw malformed("context reconstruction");
  }
  return value as unknown as ContextDetail;
}

function isConfidence(value: unknown): boolean {
  return typeof value === "string" && confidenceLevels.has(value);
}

function asDoctor(value: unknown): DoctorReport {
  const validItem = (item: unknown) =>
    isRecord(item) &&
    typeof item.label === "string" &&
    typeof item.source === "string" &&
    typeof item.tokens === "number";
  const validDuplicate = (finding: unknown) =>
    isRecord(finding) &&
    typeof finding.copies === "number" &&
    typeof finding.totalTokens === "number" &&
    typeof finding.repeatedTokens === "number" &&
    typeof finding.share === "number" &&
    isConfidence(finding.confidence) &&
    Array.isArray(finding.items) &&
    finding.items.every(validItem);
  const validLowEntropy = (finding: unknown) =>
    isRecord(finding) &&
    typeof finding.label === "string" &&
    typeof finding.source === "string" &&
    typeof finding.tokens === "number" &&
    typeof finding.compressionRatio === "number" &&
    typeof finding.wasteScoreTokens === "number" &&
    typeof finding.share === "number" &&
    isConfidence(finding.confidence);
  const validSecret = (finding: unknown) =>
    isRecord(finding) &&
    typeof finding.kind === "string" &&
    typeof finding.occurrences === "number" &&
    isNumberOrNull(finding.turn) &&
    typeof finding.line === "number" &&
    typeof finding.eventType === "string";

  if (
    !isRecord(value) ||
    typeof value.turn !== "number" ||
    typeof value.duplicateGroups !== "number" ||
    typeof value.repeatedTokens !== "number" ||
    !Array.isArray(value.duplicates) ||
    !value.duplicates.every(validDuplicate) ||
    typeof value.lowEntropyItems !== "number" ||
    typeof value.wasteScoreTokens !== "number" ||
    !Array.isArray(value.lowEntropy) ||
    !value.lowEntropy.every(validLowEntropy) ||
    typeof value.secretFindings !== "number" ||
    typeof value.secretOccurrences !== "number" ||
    typeof value.scannedRecords !== "number" ||
    typeof value.unreadableRecords !== "number" ||
    !Array.isArray(value.secrets) ||
    !value.secrets.every(validSecret) ||
    typeof value.unmeasuredItems !== "number"
  ) {
    throw malformed("Context Doctor");
  }
  return value as unknown as DoctorReport;
}

function asLifecycle(value: unknown): LifecycleReport {
  const validDeparture = (departure: unknown) =>
    departure === null ||
    (isRecord(departure) &&
      typeof departure.kind === "string" &&
      ["compaction", "branch-diverged", "unexplained"].includes(departure.kind) &&
      isNumberOrNull(departure.turn) &&
      isNumberOrNull(departure.reclaimed));
  if (
    !isRecord(value) ||
    typeof value.id !== "string" ||
    typeof value.label !== "string" ||
    typeof value.category !== "string" ||
    typeof value.source !== "string" ||
    !isNumberOrNull(value.firstPresent) ||
    !isNumberOrNull(value.lastPresent) ||
    typeof value.turnsPresent !== "number" ||
    !Array.isArray(value.runs) ||
    !value.runs.every(
      (run) =>
        isRecord(run) &&
        typeof run.from === "number" &&
        typeof run.to === "number" &&
        typeof run.turns === "number",
    ) ||
    !validDeparture(value.departure) ||
    typeof value.stillPresent !== "boolean" ||
    !Array.isArray(value.unknownTurns) ||
    !value.unknownTurns.every((turn) => typeof turn === "number") ||
    typeof value.scannedTurns !== "number" ||
    typeof value.otherThreadTurns !== "number" ||
    !isNumberOrNull(value.lastScannedTurn) ||
    !isNumberOrNull(value.recordedFirstSeen) ||
    typeof value.firstSeenDisagrees !== "boolean"
  ) {
    throw malformed("item lifecycle");
  }
  return value as unknown as LifecycleReport;
}

export function getStartup(): Promise<StartupSummary> {
  if (!inTauri()) return Promise.resolve(demoStartup);
  return invoke<unknown>("get_startup").then(asStartup);
}

/**
 * Search session metadata on the backend and return explicit paging facts.
 *
 * `refresh` marks a genuine catalog refresh (the sidebar's refresh button):
 * only that case asks the backend to treat its parsed-session cache as
 * possibly stale. An ordinary query, filter, or "Load more" page must not
 * set it — doing so would throw away parsing the user just waited for.
 */
export function searchSessions(
  agent?: string,
  query?: string,
  offset = 0,
  limit = 200,
  refresh = false,
): Promise<SessionPage> {
  if (!inTauri()) {
    const needle = query?.trim().toLocaleLowerCase();
    const matches = demoSessions.filter(
      (session) =>
        (!agent || session.agent === agent) &&
        (!needle ||
          [session.project, session.id, session.path, session.agent]
            .filter(Boolean)
            .some((value) => value!.toLocaleLowerCase().includes(needle))),
    );
    const sessions = matches.slice(offset, offset + limit);
    return Promise.resolve({
      sessions,
      total: matches.length,
      offset,
      hasMore: offset + sessions.length < matches.length,
    });
  }
  return invoke<unknown>("search_sessions", {
    agent: agent || null,
    query: query?.trim() || null,
    offset,
    limit,
    refresh,
  }).then(asSessionPage);
}

/**
 * The same id string can legitimately appear under two different agents, so
 * every per-session lookup below takes `agent` alongside `id` rather than
 * trusting the id alone to identify a session.
 */
export function inspectSession(agent: Agent, id: string): Promise<SessionDetail> {
  if (!inTauri()) return Promise.resolve(demoDetail(id));
  return invoke<unknown>("inspect_session", { id, agent }).then(asDetail);
}

export function getContext(agent: Agent, id: string, turn?: number): Promise<ContextDetail> {
  if (!inTauri()) return Promise.resolve(demoContext(turn));
  return invoke<unknown>("get_context", { id, agent, turn: turn ?? null }).then(asContext);
}

export function runDoctor(agent: Agent, id: string, turn?: number): Promise<DoctorReport> {
  if (!inTauri()) return Promise.resolve(demoDoctor(turn));
  return invoke<unknown>("run_doctor", { id, agent, turn: turn ?? null }).then(asDoctor);
}

export function getLifecycle(agent: Agent, id: string, item: string): Promise<LifecycleReport> {
  if (!inTauri()) return Promise.resolve(demoLifecycle(item));
  return invoke<unknown>("get_lifecycle", { id, agent, item }).then(asLifecycle);
}
