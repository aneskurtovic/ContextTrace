export type Agent = "codex" | "claude-code";

export interface RootSummary {
  agent: Agent;
  paths: string[];
}

export interface StartupSummary {
  roots: RootSummary[];
  warnings: string[];
}

export interface SessionSummary {
  id: string;
  agent: Agent;
  path: string;
  sizeBytes: number;
  project: string | null;
  startedAt: string | null;
  lastActivity: string | null;
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
