import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import type {
  Agent,
  ArchiveEntrySummary,
  ArchiveHolding,
  ArchiveIntegritySummary,
  ArchiveVerification,
  CategoryDelta,
  Comparability,
  CompactionDiff,
  CompactionDiffUnavailableReason,
  CompactionItemDisposition,
  ContextDetail,
  CostReport,
  Deliverability,
  GhostItem,
  DoctorReport,
  ExportOutcome,
  LifecycleReport,
  MemoryHit,
  NotificationPage,
  NotificationRecord,
  NotificationRuleId,
  NotificationSettings,
  NotificationStatus,
  OsDelivery,
  ResidualPoint,
  ResidualReport,
  ResidualStep,
  SessionDetail,
  SessionPage,
  SessionSummary,
  SessionTitle,
  SessionUpdatedEvent,
  StartupSummary,
  InstructionFileComparison,
  InstructionFileReport,
  TemporalGhost,
  TestNotificationResult,
  TranscriptEntry,
  TranscriptPage,
  ThreadRole,
  ToolDelta,
  TurnDiff,
  TurnSide,
  TurnTarget,
} from "./types";
import {
  demoArchiveHolding,
  demoArchiveVerification,
  demoCompactionDiff,
  demoContext,
  demoDetail,
  demoDoctor,
  demoLifecycle,
  demoNotificationPage,
  demoNotificationSettings,
  demoNotificationStatus,
  demoResidual,
  demoSessions,
  demoStartup,
  demoTurnDiff,
  demoInstructionFiles,
  demoTemporalGhost,
  demoCost,
  demoTranscript,
  demoTranscriptEntry,
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
const redactionModes = new Set(["redacted", "raw"]);
const exportRedactionLevels = new Set(["none", "secrets"]);

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

const titleSources = new Set(['agentGenerated', 'firstPrompt']);

/** A title must state where it came from, or the row cannot say. */
function isTitle(value: unknown): value is SessionTitle | null {
  return (
    value === null ||
    (isRecord(value) &&
      typeof value.text === 'string' &&
      typeof value.source === 'string' &&
      titleSources.has(value.source))
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
    isTitle(value.title) &&
    isStringOrNull(value.gitBranch) &&
    isStringOrNull(value.startedAt) &&
    isStringOrNull(value.lastActivity) &&
    isThreadRole(value.threadRole)
  );
}

const transcriptKinds = new Set([
  'user',
  'assistant',
  'reasoning',
  'toolCall',
  'toolResult',
  'injection',
  'compaction',
]);

function isTranscriptEntry(value: unknown): value is TranscriptEntry {
  return (
    isRecord(value) &&
    typeof value.index === 'number' &&
    typeof value.kind === 'string' &&
    transcriptKinds.has(value.kind) &&
    isNumberOrNull(value.turn) &&
    isStringOrNull(value.label) &&
    typeof value.text === 'string' &&
    typeof value.truncated === 'boolean' &&
    // `chars` and `truncated` are what a collapsed row states about the text it
    // is not showing. A row that omitted either would be claiming a size it
    // never measured.
    isNumberOrNull(value.chars) &&
    typeof value.sidechain === 'boolean' &&
    typeof value.error === 'boolean' &&
    typeof value.line === 'number' &&
    typeof value.collapsed === 'boolean'
  );
}

function asTranscriptPage(value: unknown): TranscriptPage {
  if (
    !isRecord(value) ||
    !Array.isArray(value.entries) ||
    !value.entries.every(isTranscriptEntry) ||
    typeof value.total !== 'number' ||
    typeof value.offset !== 'number' ||
    typeof value.hasMore !== 'boolean'
  ) {
    throw malformed('a session transcript');
  }
  return value as unknown as TranscriptPage;
}

function asTranscriptEntry(value: unknown): TranscriptEntry {
  if (!isTranscriptEntry(value)) throw malformed('a transcript entry');
  return value;
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
    !value.warnings.every((warning) => typeof warning === "string") ||
    // Required, not optional: an absent written root would render as
    // `undefined` exactly where this tool's privacy claim names the one
    // directory it writes to.
    typeof value.archiveRoot !== "string"
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
    ) ||
    !Array.isArray(value.items) ||
    !value.items.every(
      (item) =>
        isRecord(item) &&
        typeof item.id === "string" &&
        typeof item.label === "string" &&
        typeof item.category === "string" &&
        typeof item.source === "string" &&
        typeof item.tokens === "number" &&
        typeof item.share === "number" &&
        typeof item.confidence === "string" &&
        confidenceLevels.has(item.confidence) &&
        isNumberOrNull(item.firstSeenTurn) &&
        isStringOrNull(item.preview),
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

/** One window of a session's conversation. Paged: a session can be 6.8 MB. */
export function getTranscript(
  agent: Agent,
  id: string,
  offset = 0,
  limit = 40,
): Promise<TranscriptPage> {
  if (!inTauri()) return Promise.resolve(demoTranscript(offset, limit));
  return invoke<unknown>('get_transcript', { id, agent, offset, limit }).then(asTranscriptPage);
}

/** One entry in full, for an entry the reader expanded. */
export function getTranscriptEntry(
  agent: Agent,
  id: string,
  index: number,
): Promise<TranscriptEntry> {
  if (!inTauri()) return Promise.resolve(demoTranscriptEntry(index));
  return invoke<unknown>('get_transcript_entry', { id, agent, index }).then(asTranscriptEntry);
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
    typeof value.id === "string" &&
    // Checked against the known set, not just `typeof`: `agentLabel` falls
    // back to "Claude Code" for anything that isn't literally `"codex"`, so
    // an unrecognised agent string here would silently mislabel a side --
    // the exact "two different sessions presented as one" failure this field
    // exists to prevent.
    typeof value.agent === "string" &&
    agents.has(value.agent) &&
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

function asMemoryHits(value: unknown): MemoryHit[] {
  if (!Array.isArray(value)) throw malformed("memory search results");
  return value.map((item) => {
    if (!isRecord(item) || typeof item.sessionId !== "string" || typeof item.agent !== "string" || !agents.has(item.agent as Agent) ||
      !(typeof item.project === "string" || item.project === null) || typeof item.line !== "number" ||
      !(typeof item.turn === "number" || item.turn === null) || typeof item.preview !== "string") {
      throw malformed("memory search result");
    }
    return item as unknown as MemoryHit;
  });
}

export function searchMemory(agent: Agent | undefined, query: string, limit = 50): Promise<MemoryHit[]> {
  if (!inTauri()) return Promise.resolve([]);
  return invoke<unknown>("search_memory", { agent: agent ?? null, query, limit }).then(asMemoryHits);
}

function isGhostItem(value: unknown): value is GhostItem {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.label === "string" &&
    typeof value.category === "string" &&
    typeof value.source === "string" &&
    isNumberOrNull(value.leftTokens) &&
    isNumberOrNull(value.rightTokens) &&
    isNumberOrNull(value.tokenDelta) &&
    typeof value.meaningfulTokenDelta === "boolean" &&
    typeof value.confidence === "string" &&
    confidenceLevels.has(value.confidence)
  );
}

function asTemporalGhost(value: unknown): TemporalGhost {
  if (!isRecord(value)) throw malformed("the temporal ghost");
  if (value.status === "unavailable") {
    if (
      typeof value.leftTurn === "number" &&
      typeof value.rightTurn === "number" &&
      typeof value.reason === "string"
    ) {
      return value as unknown as TemporalGhost;
    }
  }
  if (
    value.status === "available" &&
    typeof value.leftTurn === "number" &&
    typeof value.rightTurn === "number" &&
    isComparability(value.comparability) &&
    Array.isArray(value.gained) &&
    value.gained.every(isGhostItem) &&
    Array.isArray(value.retained) &&
    value.retained.every(isGhostItem) &&
    Array.isArray(value.removed) &&
    value.removed.every(isGhostItem) &&
    Array.isArray(value.assumptions) &&
    value.assumptions.every((item) => typeof item === "string")
  ) {
    return value as unknown as TemporalGhost;
  }
  throw malformed("the temporal ghost");
}

function isInstructionFileComparison(value: unknown): value is InstructionFileComparison {
  return (
    isRecord(value) &&
    typeof value.path === "string" &&
    isNumberOrNull(value.turn) &&
    typeof value.line === "number" &&
    typeof value.status === "string" &&
    ["matching", "changed", "missing", "unreadable", "recordedBodyUnavailable"].includes(
      value.status,
    ) &&
    isStringOrNull(value.recordedDigest) &&
    isStringOrNull(value.currentDigest) &&
    typeof value.recordedChars === "number" &&
    isNumberOrNull(value.currentChars) &&
    typeof value.comparisonBasis === "string" &&
    isStringOrNull(value.detail)
  );
}

function asInstructionFiles(value: unknown): InstructionFileReport {
  if (
    !isRecord(value) ||
    typeof value.sessionId !== "string" ||
    !isStringOrNull(value.projectRoot) ||
    !Array.isArray(value.comparisons) ||
    !value.comparisons.every(isInstructionFileComparison) ||
    typeof value.refusalCount !== "number"
  ) {
    throw malformed("instruction-file comparison");
  }
  return value as unknown as InstructionFileReport;
}

function asCost(value: unknown): CostReport {
  if (
    !isRecord(value) ||
    typeof value.sessionId !== "string" ||
    typeof value.pricingVersion !== "string" ||
    typeof value.pricingSource !== "string" ||
    typeof value.warning !== "string" ||
    !Array.isArray(value.categories) ||
    typeof value.total !== "number" ||
    !Array.isArray(value.turns) ||
    !Array.isArray(value.unpriced) ||
    !(value.forecast === null || isRecord(value.forecast))
  ) {
    throw malformed("cost report");
  }
  return value as unknown as CostReport;
}

const notificationRuleIds: NotificationRuleId[] = [
  'contextPressure',
  'promptSpike',
  'toolErrorStreak',
  'secretExposure',
  'formatDrift',
  'compaction',
  'contextDominance',
  'residualStep',
  'instructionDrift',
  'contextWaste',
  'costBudget',
];
const notificationRuleIdSet = new Set(notificationRuleIds);
const notificationSeverities = new Set(['info', 'warning', 'critical']);
const notificationDeliveries = new Set(['off', 'feed', 'feedAndOs']);
const notificationPermissions = new Set(['granted', 'denied', 'prompt', 'unsupported']);

function isNotificationRuleId(value: unknown): value is NotificationRuleId {
  return typeof value === 'string' && notificationRuleIdSet.has(value as NotificationRuleId);
}

function isNotificationRules(value: unknown): boolean {
  if (!isRecord(value)) return false;
  return notificationRuleIds.every((ruleId) => {
    const setting = value[ruleId];
    return (
      isRecord(setting) &&
      typeof setting.delivery === 'string' &&
      notificationDeliveries.has(setting.delivery) &&
      isNumberOrNull(setting.threshold)
    );
  });
}

function asNotificationSettings(value: unknown): NotificationSettings {
  if (
    !isRecord(value) ||
    typeof value.enabled !== 'boolean' ||
    typeof value.onboardingComplete !== 'boolean' ||
    typeof value.subagentOsNotifications !== 'boolean' ||
    !isNotificationRules(value.rules) ||
    !isNumberOrNull(value.costBudgetUsd)
  ) {
    throw malformed('notification settings');
  }
  return value as unknown as NotificationSettings;
}

/**
 * A deliverability state must carry the app id it describes, except for
 * `unsupported`, where there is no identity to name. Checked rather than
 * trusted for the same reason as everything else here: the panel renders this
 * as a claim about whether the user will ever see a toast.
 */
function isDeliverability(value: unknown): value is Deliverability {
  if (!isRecord(value) || typeof value.state !== 'string') return false;
  switch (value.state) {
    case 'ready':
      return typeof value.appId === 'string';
    case 'unregistered':
      return typeof value.appId === 'string' && isStringOrNull(value.exeDir);
    case 'unsupported':
      return true;
    default:
      return false;
  }
}

function isOsDelivery(value: unknown): value is OsDelivery {
  if (!isRecord(value) || typeof value.status !== 'string') return false;
  switch (value.status) {
    case 'notRequested':
    case 'delivered':
      return true;
    case 'failed':
      return typeof value.reason === 'string';
    default:
      return false;
  }
}

function asNotificationStatus(value: unknown): NotificationStatus {
  if (
    !isRecord(value) ||
    typeof value.monitoring !== 'boolean' ||
    typeof value.osPermission !== 'string' ||
    !notificationPermissions.has(value.osPermission) ||
    !isStringOrNull(value.lastSuccessfulPoll) ||
    !isStringOrNull(value.error) ||
    !isDeliverability(value.deliverability) ||
    !isStringOrNull(value.obstacle)
  ) {
    throw malformed('notification status');
  }
  return value as unknown as NotificationStatus;
}

function asTestNotificationResult(value: unknown): TestNotificationResult {
  if (
    !isRecord(value) ||
    typeof value.delivered !== 'boolean' ||
    !isStringOrNull(value.reason) ||
    !isDeliverability(value.deliverability)
  ) {
    throw malformed('a test notification result');
  }
  return value as unknown as TestNotificationResult;
}

function asNotificationRecord(value: unknown): NotificationRecord {
  if (
    !isRecord(value) ||
    typeof value.id !== 'string' ||
    !isNotificationRuleId(value.ruleId) ||
    typeof value.severity !== 'string' ||
    !notificationSeverities.has(value.severity) ||
    typeof value.delivery !== 'string' ||
    !notificationDeliveries.has(value.delivery) ||
    !isConfidence(value.confidence) ||
    typeof value.title !== 'string' ||
    typeof value.description !== 'string' ||
    typeof value.occurredAt !== 'string' ||
    typeof value.detectedAt !== 'string' ||
    !isStringOrNull(value.readAt) ||
    !isStringOrNull(value.dismissedAt) ||
    typeof value.catchUp !== 'boolean' ||
    !isOsDelivery(value.osDelivery) ||
    !isRecord(value.location) ||
    typeof value.location.agent !== 'string' ||
    !agents.has(value.location.agent) ||
    typeof value.location.sessionId !== 'string' ||
    !isStringOrNull(value.location.project) ||
    !isNumberOrNull(value.location.turn) ||
    !isNumberOrNull(value.location.sourceLine)
  ) {
    throw malformed('a notification');
  }
  return value as unknown as NotificationRecord;
}

function asNotificationPage(value: unknown): NotificationPage {
  if (
    !isRecord(value) ||
    !Array.isArray(value.notifications) ||
    !value.notifications.every((notification) => {
      try {
        asNotificationRecord(notification);
        return true;
      } catch {
        return false;
      }
    }) ||
    !isStringOrNull(value.nextCursor) ||
    typeof value.unreadCount !== 'number' ||
    !Number.isInteger(value.unreadCount) ||
    value.unreadCount < 0
  ) {
    throw malformed('notification history');
  }
  return value as unknown as NotificationPage;
}

function asSessionUpdated(value: unknown): SessionUpdatedEvent {
  if (
    !isRecord(value) ||
    typeof value.agent !== 'string' ||
    !agents.has(value.agent) ||
    typeof value.sessionId !== 'string'
  ) {
    throw malformed('a session update event');
  }
  return value as unknown as SessionUpdatedEvent;
}

/**
 * Compare a turn against another turn -- of the same session or a different
 * one. `left`/`right` each carry their own `agent`/`id` rather than sharing
 * one, because the right side is no longer guaranteed to be the session the
 * left side came from.
 */
export function getTurnDiff(left: TurnTarget, right: TurnTarget): Promise<TurnDiff> {
  if (!inTauri()) return Promise.resolve(demoTurnDiff(left, right));
  return invoke<unknown>("get_turn_diff", {
    leftId: left.id,
    leftAgent: left.agent,
    leftTurn: left.turn,
    rightId: right.id,
    rightAgent: right.agent,
    rightTurn: right.turn,
  }).then(asTurnDiff);
}

export function getInstructionFiles(agent: Agent, id: string): Promise<InstructionFileReport> {
  if (!inTauri()) return Promise.resolve(demoInstructionFiles(id));
  return invoke<unknown>("get_instruction_files", { id, agent }).then(asInstructionFiles);
}

export function getCost(
  agent: Agent,
  id: string,
  pricing: string | null,
  forecastTurns: number | null,
): Promise<CostReport> {
  if (!inTauri()) return Promise.resolve(demoCost(id, forecastTurns ?? 0));
  return invoke<unknown>("get_cost", {
    id,
    agent,
    pricing,
    forecastTurns,
  }).then(asCost);
}

export function getTemporalGhost(
  agent: Agent,
  id: string,
  leftTurn: number,
  rightTurn: number,
): Promise<TemporalGhost> {
  if (!inTauri()) return Promise.resolve(demoTemporalGhost(leftTurn, rightTurn));
  return invoke<unknown>("get_temporal_ghost", {
    id,
    agent,
    leftTurn,
    rightTurn,
  }).then(asTemporalGhost);
}

function isResidualPoint(value: unknown): value is ResidualPoint {
  return (
    isRecord(value) &&
    typeof value.turn === "number" &&
    typeof value.promptTokens === "number" &&
    typeof value.accounted === "number" &&
    // `null` is a real, load-bearing value here: this turn's reconstruction
    // exceeded its own prompt. Coercing it would manufacture a zero remainder.
    isNumberOrNull(value.unlogged) &&
    typeof value.items === "number"
  );
}

function isResidualStep(value: unknown): value is ResidualStep {
  return (
    isRecord(value) &&
    typeof value.turn === "number" &&
    typeof value.from === "number" &&
    typeof value.to === "number" &&
    typeof value.growth === "number" &&
    typeof value.nearCompaction === "boolean"
  );
}

/**
 * Each arm checks only the fields that arm carries, so a refusal cannot arrive
 * wearing a fitted report's shape. In particular `overCounted` has no `points`
 * and no ratio confidence: accepting a payload that carried them would let the
 * panel fall back to charting a series the backend declined to stand behind.
 */
function asResidual(value: unknown): ResidualReport {
  if (isRecord(value)) {
    if (value.kind === "fitted") {
      if (
        typeof value.charsPerToken === "number" &&
        typeof value.pairsUsed === "number" &&
        typeof value.dispersion === "number" &&
        isNumberOrNull(value.unloggedOverhead) &&
        typeof value.turnsMeasured === "number" &&
        typeof value.overCountedTurns === "number" &&
        typeof value.stepThreshold === "number" &&
        confidenceLevels.has(value.promptConfidence as string) &&
        confidenceLevels.has(value.remainderConfidence as string) &&
        Array.isArray(value.points) &&
        value.points.every(isResidualPoint) &&
        Array.isArray(value.steps) &&
        value.steps.every(isResidualStep)
      ) {
        return value as unknown as ResidualReport;
      }
    }
    if (value.kind === "overCounted") {
      if (
        typeof value.charsPerToken === "number" &&
        typeof value.pairsUsed === "number" &&
        typeof value.dispersion === "number" &&
        typeof value.turnsMeasured === "number"
      ) {
        return value as unknown as ResidualReport;
      }
    }
    if (value.kind === "agentNotFitted" && agents.has(value.agent as string)) {
      return value as unknown as ResidualReport;
    }
    if (value.kind === "insufficientGrowth" && typeof value.turnsWithUsage === "number") {
      return value as unknown as ResidualReport;
    }
  }
  throw malformed("the unlogged-context measurement");
}

/**
 * Measure the context this session's agent never wrote down.
 *
 * Separate from `inspectSession` because it is not free: the backend
 * reconstructs every turn to produce the series. The panel asks for it when the
 * user does, the way the doctor scan does, rather than on every session click.
 */
export function getResidual(agent: Agent, id: string): Promise<ResidualReport> {
  if (!inTauri()) return Promise.resolve(demoResidual(agent, id));
  return invoke<unknown>("get_residual", { id, agent }).then(asResidual);
}

// ---- Archive & export -----------------------------------------------------

/** A write action asked for with no desktop bridge to carry it out. Thrown
 *  rather than faked, because claiming a file was written when nothing was
 *  is the one lie this tool cannot afford: its whole claim is evidence over
 *  invention, and a fabricated write is invention with a path attached. */
function demoRefusesToWrite(action: string): Error {
  return new Error(`Demo mode has nothing to write. ${action} needs the desktop app reading a local log.`);
}

function isArchiveEntry(value: unknown): value is ArchiveEntrySummary {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.agent === "string" &&
    agents.has(value.agent) &&
    isStringOrNull(value.project) &&
    typeof value.archivedAt === "string" &&
    typeof value.redaction === "string" &&
    redactionModes.has(value.redaction) &&
    typeof value.records === "number" &&
    typeof value.sourceBytes === "number" &&
    typeof value.archivedBytes === "number" &&
    typeof value.redactedRecords === "number" &&
    typeof value.redactedValues === "number" &&
    // Required: this is the flag that keeps a redacted copy from being read
    // as byte-identical to its source once the source is gone. A missing
    // value must not fall back to `undefined` reading as falsy -- that would
    // silently assert the copy matches when the backend never said so.
    typeof value.differsFromSource === "boolean"
  );
}

/**
 * Each arm checks only the fields that arm carries, mirroring `isComparability`
 * and `isThreadRole` above: `sourceGone.archiveMatchesDigest` is a required
 * boolean, not an optional one that would read as `false` -- "the copy no
 * longer matches" -- if the backend ever omitted it.
 */
function isArchiveIntegrity(value: unknown): value is ArchiveIntegritySummary {
  if (!isRecord(value)) return false;
  if (value.kind === "intact") return true;
  if (value.kind === "sourceChanged") {
    return (
      typeof value.recordedDigest === "string" &&
      typeof value.currentDigest === "string" &&
      typeof value.recordedBytes === "number" &&
      typeof value.currentBytes === "number"
    );
  }
  if (value.kind === "sourceGone") {
    return typeof value.archiveMatchesDigest === "boolean";
  }
  if (value.kind === "archiveDamaged") {
    return typeof value.recordedDigest === "string" && typeof value.currentDigest === "string";
  }
  return false;
}

function asArchiveEntrySummary(value: unknown): ArchiveEntrySummary {
  if (!isArchiveEntry(value)) throw malformed("archiving this session");
  return value;
}

function asArchiveHolding(value: unknown): ArchiveHolding {
  if (
    !isRecord(value) ||
    typeof value.root !== "string" ||
    !Array.isArray(value.entries) ||
    !value.entries.every(isArchiveEntry)
  ) {
    throw malformed("the archive");
  }
  return value as unknown as ArchiveHolding;
}

function asArchiveVerification(value: unknown): ArchiveVerification {
  if (
    !isRecord(value) ||
    !isArchiveIntegrity(value.integrity) ||
    // Not re-derived from `integrity`: the domain owns `copy_is_sound` and
    // `rebuildable`, and a required-boolean check here is what keeps a
    // missing field from silently reading as "false" -- which for
    // `copyIsSound` would understate exactly the case (`sourceGone`) this
    // feature exists to get right.
    typeof value.copyIsSound !== "boolean" ||
    typeof value.rebuildable !== "boolean"
  ) {
    throw malformed("verifying this archived session");
  }
  return value as unknown as ArchiveVerification;
}

function asExportOutcome(value: unknown): ExportOutcome {
  if (
    !isRecord(value) ||
    typeof value.path !== "string" ||
    typeof value.bytes !== "number" ||
    typeof value.records !== "number" ||
    typeof value.redaction !== "string" ||
    !exportRedactionLevels.has(value.redaction) ||
    typeof value.redactions !== "number"
  ) {
    throw malformed("exporting this session");
  }
  return value as unknown as ExportOutcome;
}

/** Everything currently held in the archive, most recently archived first. */
export function archivedSessions(): Promise<ArchiveHolding> {
  if (!inTauri()) return Promise.resolve(demoArchiveHolding);
  return invoke<unknown>("archived_sessions").then(asArchiveHolding);
}

/**
 * Copy one session into the archive. `raw` opts out of the default
 * redaction and must be requested explicitly by whatever calls this --
 * there is no path here that defaults to it.
 *
 * Refuses outright with no Tauri bridge: archiving is this tool's first
 * write, and demo mode has no real session to copy and no real directory to
 * copy it into. A caller should keep the control that reaches this disabled
 * in demo mode; the refusal below is the backstop, not the primary guard.
 */
export function archiveSession(agent: Agent, id: string, raw: boolean): Promise<ArchiveEntrySummary> {
  if (!inTauri()) return Promise.reject(demoRefusesToWrite("Archiving a session"));
  return invoke<unknown>("archive_session", { id, agent, raw }).then(asArchiveEntrySummary);
}

/** Re-check one archived session against the world: has its source changed,
 *  vanished, or has the copy itself gone bad. */
export function verifyArchived(agent: Agent, id: string): Promise<ArchiveVerification> {
  if (!inTauri()) return Promise.resolve(demoArchiveVerification(agent, id));
  return invoke<unknown>("verify_archived", { id, agent }).then(asArchiveVerification);
}

/**
 * Write one session out as NDJSON. `redactSecrets` mirrors `ct export
 * --redact-secrets`; unset is the CLI's own default (`ExportRedaction::None`),
 * so this reaches for *no* redaction unless a caller asks otherwise -- the
 * opposite default from `archiveSession` above, which is intentional and
 * documented where the export control renders it.
 *
 * Refuses with no Tauri bridge, for the same reason `archiveSession` does:
 * there is no local file for demo mode to write.
 */
export function exportSession(agent: Agent, id: string, redactSecrets: boolean): Promise<ExportOutcome> {
  if (!inTauri()) return Promise.reject(demoRefusesToWrite("Exporting a session"));
  return invoke<unknown>("export_session", { id, agent, redactSecrets }).then(asExportOutcome);
}

// ---- Notifications -------------------------------------------------------

export function getNotificationSettings(): Promise<NotificationSettings> {
  if (!inTauri()) return Promise.resolve(structuredClone(demoNotificationSettings));
  return invoke<unknown>('get_notification_settings').then(asNotificationSettings);
}

export function updateNotificationSettings(
  settings: NotificationSettings,
): Promise<NotificationSettings> {
  if (!inTauri()) return Promise.resolve(structuredClone(settings));
  return invoke<unknown>('update_notification_settings', { settings }).then(asNotificationSettings);
}

export function getNotificationStatus(): Promise<NotificationStatus> {
  if (!inTauri()) return Promise.resolve({ ...demoNotificationStatus });
  return invoke<unknown>('get_notification_status').then(asNotificationStatus);
}

/**
 * Ask for one toast now and report what happened to it.
 *
 * Without the desktop bridge there is no OS to ask, and saying "delivered"
 * would be the same fabrication this whole command exists to remove — so the
 * demo path answers `unsupported`, plainly undelivered.
 */
export function sendTestNotification(): Promise<TestNotificationResult> {
  if (!inTauri()) {
    return Promise.resolve({
      delivered: false,
      reason: 'The desktop bridge is not available, so no notification was sent.',
      deliverability: { state: 'unsupported' },
    });
  }
  return invoke<unknown>('send_test_notification').then(asTestNotificationResult);
}

export function listNotifications(
  beforeId: string | null = null,
  limit = 30,
  unreadOnly = false,
): Promise<NotificationPage> {
  if (!inTauri()) return Promise.resolve(structuredClone(demoNotificationPage));
  return invoke<unknown>('list_notifications', { beforeId, limit, unreadOnly }).then(asNotificationPage);
}

export function markNotificationsRead(ids: string[] | null = null): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('mark_notifications_read', { ids });
}

export function dismissNotification(id: string): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('dismiss_notification', { id });
}

export function clearNotificationHistory(): Promise<void> {
  if (!inTauri()) return Promise.resolve();
  return invoke<void>('clear_notification_history');
}

/** Subscribe to durable feed changes. Commands remain authoritative; callers re-list. */
export async function listenForNotificationUpdates(callback: () => void): Promise<UnlistenFn> {
  if (!inTauri()) return () => undefined;
  const notify = (payload: unknown) => {
    asNotificationRecord(payload);
    callback();
  };
  const [unlistenCreated, unlistenUpdated] = await Promise.all([
    listen<unknown>('contexttrace://notification-created', (event) => notify(event.payload)),
    listen<unknown>('contexttrace://notification-updated', (event) => notify(event.payload)),
  ]);
  return () => {
    unlistenCreated();
    unlistenUpdated();
  };
}

export function listenForSessionUpdates(
  callback: (event: SessionUpdatedEvent) => void,
): Promise<UnlistenFn> {
  if (!inTauri()) return Promise.resolve(() => undefined);
  return listen<unknown>('contexttrace://session-updated', (event) => {
    callback(asSessionUpdated(event.payload));
  });
}
