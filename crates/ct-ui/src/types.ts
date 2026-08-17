export type Agent = "codex" | "claude-code";

export interface RootSummary {
  agent: Agent;
  paths: string[];
}

export interface StartupSummary {
  roots: RootSummary[];
  warnings: string[];
  /** The one directory ContextTrace writes to -- `ct roots`' own figure for
   *  it. `roots` above lists directories this tool only ever reads; the
   *  written directory belongs in the same disclosure, distinguished as
   *  written rather than read, not folded into `roots` as though archiving
   *  were another source this tool merely observes. */
  archiveRoot: string;
}

/**
 * Where a session sits in its thread group.
 *
 * A discriminated union rather than `{ kind, parent: string | null }`, so the
 * pairing the backend guarantees is the pairing the compiler enforces: a
 * `"root"` with a parent attached, or a `"subagent"` without one, cannot be
 * written here at all. The looser shape typechecks every call site through a
 * `parent ?? ""` fallback that can never fire — a branch no test can reach and
 * no reader can justify. This mirrors the Rust `ThreadRole`, where
 * `Subagent { parent }` carries its parent in the variant for the same reason.
 */
export type ThreadRole =
  | { kind: "root"; parent: null }
  | { kind: "subagent"; parent: string };

export interface SessionSummary {
  id: string;
  agent: Agent;
  path: string;
  sizeBytes: number;
  project: string | null;
  startedAt: string | null;
  lastActivity: string | null;
  threadRole: ThreadRole;
}

/**
 * Whether two turns' token figures may be subtracted, and how far.
 *
 * A discriminated union because these are not degrees of one claim:
 * `identical` means the subtraction is exact, `skewed` means exact to within
 * a stated bound, and `incomparable` means no bound exists at all. That third
 * case is emphatically not "skew of zero" — zero is the *strongest* claim
 * available, so a `skew: number | null` shape would let a missing bound fall
 * back to asserting perfect comparability.
 */
export type Comparability =
  | { kind: "identical"; estimator: string }
  | { kind: "skewed"; left: string; right: string; skew: number }
  | { kind: "incomparable"; left: string; right: string; reason: string };

/**
 * One side of a turn comparison. Carries `id`/`agent` so the two sides of a
 * cross-session diff never present as one session with two turn numbers --
 * the comparison used to be within a single already-selected session, and a
 * caller could read "which session" off the page it came from. It cannot
 * once the right side may be a different session, so the fact travels with
 * the side.
 */
export interface TurnSide {
  id: string;
  agent: Agent;
  turn: number;
  totalTokens: number;
  items: number;
  residual: number;
  confidence: Confidence;
}

export interface CategoryDelta {
  category: string;
  left: number;
  right: number;
  delta: number;
  leftItems: number;
  rightItems: number;
  itemDelta: number;
  /** `null` when no bound can be stated; `meaningful` is then false and
   *  `delta` is arithmetic with no claim attached. */
  instrumentBound: number | null;
  meaningful: boolean;
}

export interface ToolDelta {
  tool: string;
  leftCalls: number;
  rightCalls: number;
  leftTokens: number;
  rightTokens: number;
  callDelta: number;
  tokenDelta: number;
  instrumentBound: number | null;
  meaningful: boolean;
}

/** One side of a turn comparison, as a caller names it before asking: which
 *  session, and which turn of it. `TurnSide` above is the answer that comes
 *  back; this is the question. */
export interface TurnTarget {
  agent: Agent;
  id: string;
  turn: number;
}

export interface TurnDiff {
  left: TurnSide;
  right: TurnSide;
  comparability: Comparability;
  /** Read from both sides' own usage records, so it is free of the instrument
   *  question the rest of this type manages — but only when
   *  `totalsAreObserved`. */
  promptDelta: number;
  totalsAreObserved: boolean;
  categories: CategoryDelta[];
  tools: ToolDelta[];
}

export type InstructionFileStatus =
  | "matching"
  | "changed"
  | "missing"
  | "unreadable"
  | "recordedBodyUnavailable";

export interface InstructionFileComparison {
  path: string;
  turn: number | null;
  line: number;
  status: InstructionFileStatus;
  recordedDigest: string | null;
  currentDigest: string | null;
  recordedChars: number;
  currentChars: number | null;
  comparisonBasis: string;
  detail: string | null;
}

export interface InstructionFileReport {
  sessionId: string;
  projectRoot: string | null;
  comparisons: InstructionFileComparison[];
  refusalCount: number;
}

export interface CostCategory {
  name: string;
  tokens: number;
  cost: number;
  confidence: string;
}

export interface CostForecast {
  additionalTurns: number;
  averageTokensPerTurn: CostCategory[];
  projectedAdditional: number;
  projectedTotal: number;
  assumptions: string[];
}

export interface CostReport {
  sessionId: string;
  pricingVersion: string;
  pricingSource: string;
  warning: string;
  categories: CostCategory[];
  total: number;
  turns: unknown[];
  unpriced: unknown[];
  forecast: CostForecast | null;
}

export interface GhostItem {
  id: string;
  label: string;
  category: string;
  source: string;
  leftTokens: number | null;
  rightTokens: number | null;
  tokenDelta: number | null;
  meaningfulTokenDelta: boolean;
  confidence: Confidence;
}

export type TemporalGhost =
  | {
      status: "available";
      leftTurn: number;
      rightTurn: number;
      comparability: Comparability;
      gained: GhostItem[];
      retained: GhostItem[];
      removed: GhostItem[];
      assumptions: string[];
    }
  | { status: "unavailable"; leftTurn: number; rightTurn: number; reason: string };

export interface SessionPage {
  sessions: SessionSummary[];
  total: number;
  offset: number;
  hasMore: boolean;
}

export interface MemoryHit {
  sessionId: string;
  agent: Agent;
  project: string | null;
  line: number;
  turn: number | null;
  preview: string;
}

export interface CompactionSummary {
  turn: number | null;
  reclaimed: number | null;
  /** The log line this compaction was recorded on -- the identity a
   *  compaction autopsy is looked up by, because a turn number is not
   *  guaranteed unique across compactions in the same session. */
  lineNo: number;
}

export interface GrowthPoint {
  turn: number;
  promptTokens: number | null;
  compaction: CompactionSummary | null;
}

export interface SessionDetail {
  session: SessionSummary;
  source?: {
    kind: "live-log" | "archive";
    archivedAt: string | null;
    redaction: string | null;
    differsFromSource: boolean | null;
  };
  model: string | null;
  agentVersion: string | null;
  gitBranch: string | null;
  turnCount: number;
  eventCount: number;
  totalOutputTokens: number;
  peakTurn: number | null;
  peakPromptTokens: number | null;
  contextWindow: number | null;
  fidelity: number;
  unrecognisedEvents: number;
  unplacedCompactions: number;
  growth: GrowthPoint[];
}

export type Confidence = "observed" | "derived" | "estimated";

/** Stable identifiers shared by the evaluator, persisted settings, and UI. */
export type NotificationRuleId =
  | 'contextPressure'
  | 'promptSpike'
  | 'toolErrorStreak'
  | 'secretExposure'
  | 'formatDrift'
  | 'compaction'
  | 'contextDominance'
  | 'residualStep'
  | 'instructionDrift'
  | 'contextWaste'
  | 'costBudget';

export type NotificationSeverity = 'info' | 'warning' | 'critical';
export type NotificationDelivery = 'off' | 'feed' | 'feedAndOs';
export type NotificationPermission = 'granted' | 'denied' | 'prompt' | 'unsupported';

export interface NotificationRuleSetting {
  delivery: NotificationDelivery;
  /** The rule's primary editable threshold. `null` means it is not configured. */
  threshold: number | null;
}

export interface NotificationSettings {
  enabled: boolean;
  onboardingComplete: boolean;
  subagentOsNotifications: boolean;
  rules: Record<NotificationRuleId, NotificationRuleSetting>;
  costBudgetUsd: number | null;
}

export interface NotificationStatus {
  monitoring: boolean;
  osPermission: NotificationPermission;
  lastSuccessfulPoll: string | null;
  error: string | null;
}

export interface NotificationLocation {
  agent: Agent;
  sessionId: string;
  project: string | null;
  turn: number | null;
  sourceLine: number | null;
}

export interface NotificationRecord {
  id: string;
  ruleId: NotificationRuleId;
  severity: NotificationSeverity;
  delivery: NotificationDelivery;
  confidence: Confidence;
  title: string;
  /** Privacy-safe summary produced by the backend; never prompt or tool content. */
  description: string;
  occurredAt: string;
  detectedAt: string;
  readAt: string | null;
  dismissedAt: string | null;
  catchUp: boolean;
  location: NotificationLocation;
}

export interface NotificationPage {
  notifications: NotificationRecord[];
  nextCursor: string | null;
  unreadCount: number;
}

export interface SessionUpdatedEvent {
  agent: Agent;
  sessionId: string;
}

export interface CategorySummary {
  category: string;
  label: string;
  tokens: number;
  share: number;
  itemCount: number;
  confidence: Confidence;
}

export interface ContributorSummary {
  id: string;
  label: string;
  category: string;
  source: string;
  tokens: number;
  share: number;
  confidence: Confidence;
}

export interface ContextDetail {
  turn: number;
  model: string | null;
  totalTokens: number;
  residualTokens: number;
  residualIsMeaningful: boolean;
  contextWindow: number | null;
  utilisation: number | null;
  calibrationScale: number | null;
  categories: CategorySummary[];
  contributors: ContributorSummary[];
}

export interface DiagnosticItemSummary {
  label: string;
  source: string;
  tokens: number;
}

export interface DuplicateSummary {
  copies: number;
  totalTokens: number;
  repeatedTokens: number;
  share: number;
  confidence: Confidence;
  items: DiagnosticItemSummary[];
}

export interface LowEntropySummary {
  label: string;
  source: string;
  tokens: number;
  compressionRatio: number;
  wasteScoreTokens: number;
  share: number;
  confidence: Confidence;
}

export interface SecretFindingSummary {
  kind: string;
  occurrences: number;
  turn: number | null;
  line: number;
  eventType: string;
}

export interface DoctorReport {
  turn: number;
  duplicateGroups: number;
  repeatedTokens: number;
  duplicates: DuplicateSummary[];
  lowEntropyItems: number;
  wasteScoreTokens: number;
  lowEntropy: LowEntropySummary[];
  secretFindings: number;
  secretOccurrences: number;
  scannedRecords: number;
  unreadableRecords: number;
  /** Items the duplicate and low-information detectors could not examine,
   *  because their logs exposed no content to measure. An empty `duplicates`
   *  means "none found among the rest", not "none present". */
  unmeasuredItems: number;
  secrets: SecretFindingSummary[];
}

export interface TurnRunSummary {
  from: number;
  to: number;
  turns: number;
}

export interface DepartureSummary {
  kind: "compaction" | "branch-diverged" | "unexplained";
  turn: number | null;
  reclaimed: number | null;
}

export interface LifecycleReport {
  id: string;
  label: string;
  category: string;
  source: string;
  firstPresent: number | null;
  lastPresent: number | null;
  turnsPresent: number;
  runs: TurnRunSummary[];
  departure: DepartureSummary | null;
  stillPresent: boolean;
  unknownTurns: number[];
  scannedTurns: number;
  otherThreadTurns: number;
  lastScannedTurn: number | null;
  recordedFirstSeen: number | null;
  firstSeenDisagrees: boolean;
}

/**
 * Where one item in a Codex compaction's replacement history sits, mirroring
 * `ct_domain::CompactionItemDisposition`.
 *
 * A discriminated union rather than a flattened
 * `{ kind, historyIndex: number | null, replacementIndex: number | null }`,
 * for the same reason as `ThreadRole` above: the Rust type names each index
 * on the variant that has it, so a preserved item's two positions (where it
 * was, where it moved to) cannot be pulled apart, and a dropped item cannot
 * carry a phantom `replacementIndex` no call site can ever read. The looser
 * shape would force a `?? 0` fallback at every render call site that could
 * never actually fire.
 */
export type CompactionItemDisposition =
  | { kind: "dropped"; historyIndex: number }
  | { kind: "preserved"; historyIndex: number; replacementIndex: number }
  | { kind: "addedByReplacement"; replacementIndex: number };

/** One item participating in a compaction's replacement, mirroring
 *  `ct_domain::CompactionDiffItem`. */
export interface CompactionDiffItem {
  /** Codex Responses API item type, e.g. `message` or `function_call_output`. */
  itemType: string;
  /** Present only on message items. */
  role: string | null;
  disposition: CompactionItemDisposition;
  /** Size after serializing the parsed JSON item into a normalized compact
   *  representation -- derived, not a token estimate or the original wire
   *  size. */
  normalizedJsonBytes: number;
  /** A tokenizer measurement only when the complete model-visible item was
   *  plain text. `null` for opaque blobs and structured outputs -- never
   *  coerced to zero, which would misrepresent "not measured" as "measured
   *  as empty". */
  textTokens: number | null;
  confidence: Confidence;
}

/** Why one specific compaction's structural diff could not be produced, even
 *  though this agent generally records replacement history. Mirrors
 *  `ct_domain::CompactionDiffUnavailable`. */
export type CompactionDiffUnavailableReason =
  | "missingReplacementHistory"
  | "oversizedRawLine"
  | "unavailableRawLine"
  | "malformedRawLine"
  | "malformedPrecedingItem"
  | "unknownPrecedingHistory";

/**
 * The outcome of asking for one compaction's structural diff.
 *
 * Three cases, not a `{ items: [], error: string | null }` shape: `available`
 * mirrors `ct_domain::CompactionDiff::Available`; `unavailable` mirrors
 * `CompactionDiff::Unavailable` -- one specific compaction's evidence could
 * not be read on an agent that generally supports this; `unsupported` is a
 * fact `CompactionDiff` cannot express at all -- the agent (Claude Code)
 * never records a literal replacement history for any of its compactions,
 * which fails at the adapter itself before any single compaction is
 * considered. Collapsing that distinction into one error string would make
 * "this specific compaction's raw line is corrupted" and "this agent never
 * had this evidence to begin with" look like the same failure, which they
 * are not -- and only the panel that keeps them apart can tell a Claude Code
 * user the evidence does not exist rather than that the feature is broken.
 */
export type CompactionDiff =
  | { status: "available"; turn: number | null; lineNo: number; items: CompactionDiffItem[] }
  | {
      status: "unavailable";
      turn: number | null;
      lineNo: number;
      reason: CompactionDiffUnavailableReason;
    }
  | { status: "unsupported"; detail: string };

/**
 * One turn's account of its own prompt, mirroring
 * `ct_application::ResidualPoint`.
 *
 * `unlogged` is `null` where the reconstructed content already exceeds the
 * prompt the agent reported for that turn, and that has to stay distinguishable
 * from zero. A zero would assert a complete inventory of the context — the one
 * claim this measurement exists to avoid making — so a renderer must draw a gap
 * there, never a point on the axis.
 */
export interface ResidualPoint {
  turn: number;
  /** The agent's own figure for this turn. Observed. */
  promptTokens: number;
  /** What the reconstructed content accounts for, at the fitted ratio. */
  accounted: number;
  unlogged: number | null;
  items: number;
}

/**
 * A change in the unlogged remainder large enough, and sustained enough, to
 * mean the harness altered the prompt's hidden part. Mirrors
 * `ct_application::ResidualStep`.
 */
export interface ResidualStep {
  turn: number;
  from: number;
  to: number;
  /** Signed: a rise means the prompt gained content the log does not record. */
  growth: number;
  /**
   * A compaction close enough to this step to account for it. Decided on the
   * backend against the session's own compaction events, not re-derived here:
   * a step the log already explains must not also be narrated as an unrecorded
   * harness change, which would invent a second cause for one event.
   */
  nearCompaction: boolean;
}

/**
 * What one session can say about the context its agent never wrote down.
 *
 * Four cases rather than a fitted report with nullable figures, because three
 * of them are refusals with different causes and a caller must not be able to
 * paper over them. `agentNotFitted` says the measurement does not apply to this
 * agent at all; `insufficientGrowth` says this session never grew enough to
 * measure a ratio from; `overCounted` says a ratio was fitted but every turn's
 * reconstruction exceeded its own prompt, so no remainder can be read out of
 * the subtraction.
 *
 * That last one is the case this shape exists for. A session that over-counts
 * still yields a ratio, so a `fitted`-shaped report with an all-`null` series
 * would render a confident header — a measured characters-per-token figure —
 * above an empty chart. Splitting it out makes the refusal the answer instead
 * of an absence the reader has to notice. A session where only *some* turns
 * over-count stays `fitted` and states how many, which is a measurement rather
 * than a gap.
 */
export type ResidualReport =
  | {
      kind: "fitted";
      charsPerToken: number;
      /** Consecutive-turn pairs the ratio was taken from: the sample size. */
      pairsUsed: number;
      /** Spread of those per-pair ratios as p75/p25. Near 1.0 means the session
       *  tokenizes consistently; a large value means these figures deserve
       *  less weight. */
      dispersion: number;
      /** The session's typical hidden constant. `null` when it came out
       *  negative, which is reported rather than clamped to zero. */
      unloggedOverhead: number | null;
      turnsMeasured: number;
      overCountedTurns: number;
      /** The smallest sustained change reported as a step. */
      stepThreshold: number;
      promptConfidence: Confidence;
      remainderConfidence: Confidence;
      points: ResidualPoint[];
      steps: ResidualStep[];
    }
  | { kind: "overCounted"; charsPerToken: number; pairsUsed: number; dispersion: number; turnsMeasured: number }
  | { kind: "agentNotFitted"; agent: Agent }
  | { kind: "insufficientGrowth"; turnsWithUsage: number };

/**
 * Whether an archived copy still holds the credentials its source held.
 * Mirrors `ct_domain::model::archive::RedactionMode`. Redaction is the
 * default on the write path (see `ArchiveEntrySummary`); `"raw"` only ever
 * arrives from an explicit request, and every row states which it got so a
 * reader never has to assume.
 */
export type RedactionMode = "redacted" | "raw";

/**
 * One session as the archive's manifest records it, mirroring
 * `ct_domain::model::archive::ArchiveEntry`.
 *
 * `differsFromSource` is carried rather than re-derived from `redactedValues
 * > 0`: the domain owns that judgement (`ArchiveEntry::differs_from_source`),
 * and restating its condition here would be a second place for it to drift
 * from what the backend actually enforces.
 */
export interface ArchiveEntrySummary {
  id: string;
  agent: Agent;
  project: string | null;
  archivedAt: string;
  redaction: RedactionMode;
  records: number;
  sourceBytes: number;
  archivedBytes: number;
  redactedRecords: number;
  redactedValues: number;
  differsFromSource: boolean;
}

/**
 * What checking an archived session against the world found, mirroring
 * `ct_domain::model::archive::ArchiveIntegrity`.
 *
 * Four outcomes, not a red/green status: an intact archive needs nothing, a
 * changed source needs a re-ingest, a damaged copy needs a re-ingest *and*
 * means the previous copy cannot be trusted, and a vanished source makes this
 * copy the only evidence left -- the case archiving exists for, and the one a
 * plain "verified" badge would understate rather than state.
 */
export type ArchiveIntegritySummary =
  | { kind: "intact" }
  | {
      kind: "sourceChanged";
      recordedDigest: string;
      currentDigest: string;
      recordedBytes: number;
      currentBytes: number;
    }
  | { kind: "sourceGone"; archiveMatchesDigest: boolean }
  | { kind: "archiveDamaged"; recordedDigest: string; currentDigest: string };

/**
 * `copyIsSound` and `rebuildable` cross the wire as computed booleans rather
 * than being re-derived from `integrity` in TypeScript: they are judgements
 * the Rust domain owns (`ArchiveIntegrity::copy_is_sound`, `::rebuildable`),
 * and a second implementation here would be a second place for either to
 * drift from what the domain actually enforces.
 */
export interface ArchiveVerification {
  integrity: ArchiveIntegritySummary;
  copyIsSound: boolean;
  rebuildable: boolean;
}

/** Everything the archive holds, mirroring the `ct archive` listing.
 *  `root` is the directory copies are written to -- the same string `ct
 *  roots` prints, and the app's whole privacy claim rests on naming it.
 *  `entries` arrive most-recently-archived first; do not re-sort them. */
export interface ArchiveHolding {
  root: string;
  entries: ArchiveEntrySummary[];
}

/** What was requested when a session was exported, mirroring
 *  `ct_application::ExportRedaction`. `"none"` is the CLI's own default
 *  (`ExportRedaction::default()`); redaction on export is an opt-in, unlike
 *  the archive's default-redacted write path. */
export type ExportRedaction = "none" | "secrets";

/** The outcome of writing one session to NDJSON, mirroring
 *  `ct_application::ExportReport` plus the file facts `ct export` prints
 *  alongside it. `redactions` is how many values were actually replaced --
 *  zero whenever `redaction` is `"none"` or the session held nothing to
 *  redact, not an indication either way that the write failed. */
export interface ExportOutcome {
  path: string;
  bytes: number;
  records: number;
  redaction: ExportRedaction;
  redactions: number;
}
