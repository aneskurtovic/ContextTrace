export type Agent = "codex" | "claude-code";

export interface RootSummary {
  agent: Agent;
  paths: string[];
}

export interface StartupSummary {
  roots: RootSummary[];
  warnings: string[];
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

export interface SessionPage {
  sessions: SessionSummary[];
  total: number;
  offset: number;
  hasMore: boolean;
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
