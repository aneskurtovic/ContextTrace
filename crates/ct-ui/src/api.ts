import { invoke } from "@tauri-apps/api/core";
import type {
  Agent,
  CategoryDelta,
  Comparability,
  CompactionDiff,
  CompactionDiffUnavailableReason,
  CompactionItemDisposition,
  ContextDetail,
  DoctorReport,
  LifecycleReport,
  SessionDetail,
  SessionPage,
  SessionSummary,
  StartupSummary,
  ThreadRole,
  ToolDelta,
  TurnDiff,
  TurnSide,
} from "./types";
import {
  demoCompactionDiff,
  demoContext,
  demoDetail,
  demoDoctor,
  demoLifecycle,
  demoSessions,
  demoStartup,
  demoTurnDiff,
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

/**
 * `parent` must be a string exactly when `kind` is `"subagent"`. Checking the
 * pairing rather than each field alone is the point: the backend's
 * `ThreadRole` makes the other combination unrepresentable, and a validator
 * that only checked types independently would silently accept a payload the
 * type system on the other side of the IPC boundary cannot produce.
 */
function isThreadRole(value: unknown): value is ThreadRole {
  return (
    isRecord(value) &&
    ((value.kind === "root" && value.parent === null) ||
      (value.kind === "subagent" && typeof value.parent === "string"))
  );
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
    isStringOrNull(value.lastActivity) &&
    isThreadRole(value.threadRole)
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
            isNumberOrNull(point.compaction.reclaimed) &&
            typeof point.compaction.lineNo === "number")),
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

const compactionUnavailableReasons = new Set<CompactionDiffUnavailableReason>([
  "missingReplacementHistory",
  "oversizedRawLine",
  "unavailableRawLine",
  "malformedRawLine",
  "malformedPrecedingItem",
  "unknownPrecedingHistory",
]);

/**
 * `historyIndex`/`replacementIndex` must be present exactly where the `kind`
 * says they are. Mirrors `isThreadRole`: checking the pairing, not each
 * field independently, is what keeps this validator as strict as the Rust
 * `CompactionItemDisposition` it stands in for -- a payload with a stray
 * `replacementIndex` on a `dropped` item is not a shape the backend can
 * produce, and this must reject it rather than silently accept it.
 */
function isCompactionDisposition(value: unknown): value is CompactionItemDisposition {
  return (
    isRecord(value) &&
    ((value.kind === "dropped" && typeof value.historyIndex === "number") ||
      (value.kind === "preserved" &&
        typeof value.historyIndex === "number" &&
        typeof value.replacementIndex === "number") ||
      (value.kind === "addedByReplacement" && typeof value.replacementIndex === "number"))
  );
}

function isCompactionDiffItem(value: unknown): boolean {
  return (
    isRecord(value) &&
    typeof value.itemType === "string" &&
    isStringOrNull(value.role) &&
    isCompactionDisposition(value.disposition) &&
    typeof value.normalizedJsonBytes === "number" &&
    isNumberOrNull(value.textTokens) &&
    isConfidence(value.confidence)
  );
}

function asCompactionDiff(value: unknown): CompactionDiff {
  if (isRecord(value) && value.status === "unsupported" && typeof value.detail === "string") {
    return value as unknown as CompactionDiff;
  }
  if (
    isRecord(value) &&
    value.status === "unavailable" &&
    isNumberOrNull(value.turn) &&
    typeof value.lineNo === "number" &&
    typeof value.reason === "string" &&
    compactionUnavailableReasons.has(value.reason as CompactionDiffUnavailableReason)
  ) {
    return value as unknown as CompactionDiff;
  }
  if (
    isRecord(value) &&
    value.status === "available" &&
    isNumberOrNull(value.turn) &&
    typeof value.lineNo === "number" &&
    Array.isArray(value.items) &&
    value.items.every(isCompactionDiffItem)
  ) {
    return value as unknown as CompactionDiff;
  }
  throw malformed("compaction autopsy");
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

/**
 * The structural autopsy of one compaction, looked up by the log line its
 * chart marker carries (see `CompactionSummary.lineNo`) rather than its
 * turn, because a turn number is not guaranteed unique across compactions in
 * the same session.
 */
export function getCompactionDiff(
  agent: Agent,
  id: string,
  lineNo: number,
): Promise<CompactionDiff> {
  if (!inTauri()) return Promise.resolve(demoCompactionDiff(agent, lineNo));
  return invoke<unknown>("get_compaction_diff", { id, agent, lineNo }).then(asCompactionDiff);
}

function isComparability(value: unknown): value is Comparability {
  if (!isRecord(value)) return false;
  // Each arm names the fields that arm alone carries. A shared shape with
  // nullable members would accept `{kind:"incomparable", skew:0}` -- the one
  // combination that turns "no bound exists" into "the bound is perfect".
  if (value.kind === "identical") return typeof value.estimator === "string";
  if (value.kind === "skewed") {
    return (
      typeof value.left === "string" &&
      typeof value.right === "string" &&
      typeof value.skew === "number"
    );
  }
  if (value.kind === "incomparable") {
    return (
      typeof value.left === "string" &&
      typeof value.right === "string" &&
      typeof value.reason === "string"
    );
  }
  return false;
}

function isTurnSide(value: unknown): value is TurnSide {
  return (
    isRecord(value) &&
    typeof value.turn === "number" &&
    typeof value.totalTokens === "number" &&
    typeof value.items === "number" &&
    typeof value.residual === "number" &&
    typeof value.confidence === "string"
  );
}

function isCategoryDelta(value: unknown): value is CategoryDelta {
  return (
    isRecord(value) &&
    typeof value.category === "string" &&
    typeof value.left === "number" &&
    typeof value.right === "number" &&
    typeof value.delta === "number" &&
    typeof value.leftItems === "number" &&
    typeof value.rightItems === "number" &&
    typeof value.itemDelta === "number" &&
    isNumberOrNull(value.instrumentBound) &&
    typeof value.meaningful === "boolean"
  );
}

function isToolDelta(value: unknown): value is ToolDelta {
  return (
    isRecord(value) &&
    typeof value.tool === "string" &&
    typeof value.leftCalls === "number" &&
    typeof value.rightCalls === "number" &&
    typeof value.leftTokens === "number" &&
    typeof value.rightTokens === "number" &&
    typeof value.callDelta === "number" &&
    typeof value.tokenDelta === "number" &&
    isNumberOrNull(value.instrumentBound) &&
    typeof value.meaningful === "boolean"
  );
}

function asTurnDiff(value: unknown): TurnDiff {
  if (
    !isRecord(value) ||
    !isTurnSide(value.left) ||
    !isTurnSide(value.right) ||
    !isComparability(value.comparability) ||
    typeof value.promptDelta !== "number" ||
    typeof value.totalsAreObserved !== "boolean" ||
    !Array.isArray(value.categories) ||
    !value.categories.every(isCategoryDelta) ||
    !Array.isArray(value.tools) ||
    !value.tools.every(isToolDelta)
  ) {
    throw malformed("the turn comparison");
  }
  return value as unknown as TurnDiff;
}

export function getTurnDiff(
  agent: Agent,
  id: string,
  leftTurn: number,
  rightTurn: number,
): Promise<TurnDiff> {
  if (!inTauri()) return Promise.resolve(demoTurnDiff(leftTurn, rightTurn));
  return invoke<unknown>("get_turn_diff", { id, agent, leftTurn, rightTurn }).then(asTurnDiff);
}
