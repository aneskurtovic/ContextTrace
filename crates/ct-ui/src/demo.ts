import type {
  Agent,
  ArchiveHolding,
  ArchiveIntegritySummary,
  ArchiveVerification,
  Comparability,
  CompactionDiff,
  ContextDetail,
  CostReport,
  DoctorReport,
  GrowthPoint,
  InstructionFileReport,
  LifecycleReport,
  NotificationPage,
  NotificationSettings,
  NotificationStatus,
  ResidualPoint,
  ResidualReport,
  ResidualStep,
  SessionDetail,
  SessionSummary,
  StartupSummary,
  ThreadRole,
  TurnDiff,
  TurnTarget,
  TemporalGhost,
} from "./types";

/** The log line the demo session's one compaction marker sits on. Arbitrary
 *  but fixed, so `demoCompactionDiff` below can be looked up by the same
 *  identity a real session would use. */
const DEMO_COMPACTION_LINE_NO = 4821;

const now = Date.now();
const ago = (hours: number) => new Date(now - hours * 3_600_000).toISOString();

const root: ThreadRole = { kind: "root", parent: null };

export const demoSessions: SessionSummary[] = [
  {
    id: "0198fce2e48a7b12",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\contexttrace.jsonl",
    sizeBytes: 3_829_760,
    project: "C:\\work\\ContextTrace",
    startedAt: ago(5),
    lastActivity: ago(0.4),
    threadRole: root,
  },
  {
    id: "a30cb9e1-f9f4-4a37",
    agent: "claude-code",
    path: "C:\\Users\\demo\\.claude\\projects\\atlas\\session.jsonl",
    sizeBytes: 1_677_721,
    project: "C:\\work\\atlas-dashboard",
    startedAt: ago(30),
    lastActivity: ago(23),
    threadRole: root,
  },
  {
    id: "0198fb914e330a81",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\search.jsonl",
    sizeBytes: 812_413,
    project: "C:\\work\\semantic-search",
    startedAt: ago(51),
    lastActivity: ago(47),
    threadRole: root,
  },
  {
    id: "f485150f-0982-4876",
    agent: "claude-code",
    path: "C:\\Users\\demo\\.claude\\projects\\payments\\session.jsonl",
    sizeBytes: 7_130_317,
    project: "C:\\work\\payment-service",
    startedAt: ago(76),
    lastActivity: ago(70),
    threadRole: root,
  },
  {
    id: "0198f420c9740dac",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\compiler.jsonl",
    sizeBytes: 2_075_648,
    project: "C:\\work\\compiler-lab",
    startedAt: ago(110),
    lastActivity: ago(99),
    threadRole: root,
  },
  {
    // A demo subagent thread, so the marker this feature adds has something
    // to render without needing a real corpus to show it. Named after the
    // Codex session above rather than a new parent, matching CT-069's
    // measured shape: every local group's children point at their group's
    // root directly.
    id: "0198fce2-a9c1-4f30",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\contexttrace-subagent.jsonl",
    sizeBytes: 214_030,
    project: "C:\\work\\ContextTrace",
    startedAt: ago(5),
    lastActivity: ago(4.6),
    threadRole: { kind: "subagent", parent: "0198fce2e48a7b12" },
  },
  {
    // Deliberately short. A session this brief never grows enough for a
    // characters-per-token ratio to be measured from it, which is the ordinary
    // reason the unlogged-context view declines — and a demo that only ever
    // showed the successful fit would hide the state most sessions are in.
    id: "b71d4c08-2e55-41aa",
    agent: "claude-code",
    path: "C:\\Users\\demo\\.claude\\projects\\atlas\\quick-question.jsonl",
    sizeBytes: 48_216,
    project: "C:\\work\\atlas-dashboard",
    startedAt: ago(2),
    lastActivity: ago(1.9),
    threadRole: root,
  },
];

export const demoNotificationSettings: NotificationSettings = {
  enabled: false,
  onboardingComplete: true,
  subagentOsNotifications: false,
  costBudgetUsd: null,
  rules: {
    contextPressure: { delivery: 'feedAndOs', threshold: 75 },
    promptSpike: { delivery: 'feedAndOs', threshold: 20_000 },
    toolErrorStreak: { delivery: 'feedAndOs', threshold: 3 },
    secretExposure: { delivery: 'feedAndOs', threshold: null },
    formatDrift: { delivery: 'feedAndOs', threshold: null },
    compaction: { delivery: 'feed', threshold: null },
    contextDominance: { delivery: 'feed', threshold: 25 },
    residualStep: { delivery: 'feedAndOs', threshold: 5_000 },
    instructionDrift: { delivery: 'feedAndOs', threshold: null },
    contextWaste: { delivery: 'feed', threshold: 10_000 },
    costBudget: { delivery: 'off', threshold: null },
  },
};

export const demoNotificationStatus: NotificationStatus = {
  monitoring: false,
  osPermission: 'prompt',
  lastSuccessfulPoll: null,
  error: null,
};

export const demoNotificationPage: NotificationPage = {
  unreadCount: 2,
  nextCursor: null,
  notifications: [
    {
      id: 'demo-notification-pressure',
      ruleId: 'contextPressure',
      severity: 'warning',
      delivery: 'feedAndOs',
      confidence: 'observed',
      title: 'Context crossed 75%',
      description: 'Context utilization reached 78% at turn 32.',
      occurredAt: ago(0.3),
      detectedAt: ago(0.3),
      readAt: null,
      dismissedAt: null,
      catchUp: false,
      location: {
        agent: 'codex',
        sessionId: demoSessions[0].id,
        project: demoSessions[0].project,
        turn: 32,
        sourceLine: null,
      },
    },
    {
      id: 'demo-notification-compaction',
      ruleId: 'compaction',
      severity: 'info',
      delivery: 'feed',
      confidence: 'observed',
      title: 'Context compacted',
      description: 'Compaction reclaimed 18,400 tokens.',
      occurredAt: ago(1.1),
      detectedAt: ago(0.8),
      readAt: null,
      dismissedAt: null,
      catchUp: true,
      location: {
        agent: 'codex',
        sessionId: demoSessions[0].id,
        project: demoSessions[0].project,
        turn: 18,
        sourceLine: DEMO_COMPACTION_LINE_NO,
      },
    },
  ],
};

/** The demo sessions each residual state is attached to, so all four are
 *  reachable in the browser without a local corpus. */
const RESIDUAL_FITTED_SESSION = "a30cb9e1-f9f4-4a37";
const RESIDUAL_OVER_COUNTED_SESSION = "f485150f-0982-4876";
const RESIDUAL_SHORT_SESSION = "b71d4c08-2e55-41aa";

/** The one directory this demo pretends to write to. Named consistently with
 *  `demoArchiveHolding.root` below -- the startup panel and the archive
 *  panel are stating the same fact from two places, and a real backend
 *  would have to agree with itself too. */
const DEMO_ARCHIVE_ROOT = "C:\\Users\\demo\\AppData\\Local\\ContextTrace-archive";

export const demoStartup: StartupSummary = {
  roots: [
    { agent: "codex", paths: ["C:\\Users\\demo\\.codex\\sessions"] },
    { agent: "claude-code", paths: ["C:\\Users\\demo\\.claude\\projects"] },
  ],
  warnings: [],
  archiveRoot: DEMO_ARCHIVE_ROOT,
};

/**
 * What checking each demo archived session finds, keyed by id.
 *
 * Five entries cover the four `ArchiveIntegrity` kinds plus both readings of
 * `sourceGone`'s own boolean -- `archiveMatchesDigest: true` is the case this
 * whole feature exists for (a vanished source with a copy that still proves
 * itself), and `false` is the one situation nothing can recover from, which
 * a demo that stopped at the first `sourceGone` would never show.
 */
const DEMO_ARCHIVE_SOURCE_CHANGED_ID = "0198fb914e330a81";
const DEMO_ARCHIVE_SOURCE_GONE_SOUND_ID = "0198e0a1b2c3d4e5";
const DEMO_ARCHIVE_SOURCE_GONE_UNSOUND_ID = "0198e0f9c1a2b3d4";
const DEMO_ARCHIVE_DAMAGED_ID = "0198e155d2e3f405";

const DEMO_ARCHIVE_INTEGRITY: Record<string, ArchiveIntegritySummary> = {
  "0198fce2e48a7b12": { kind: "intact" },
  [DEMO_ARCHIVE_SOURCE_CHANGED_ID]: {
    kind: "sourceChanged",
    recordedDigest: "3f2c9a7e1d5b8046b12f0c3ae99d21a4",
    currentDigest: "8b41ff02cc7719ad4e6a0d5f8b213c77",
    recordedBytes: 640_000,
    // Matches this id's live `sizeBytes` in `demoSessions`: the session kept
    // going after it was archived, which is the ordinary reason a log grows.
    currentBytes: 812_413,
  },
  [DEMO_ARCHIVE_SOURCE_GONE_SOUND_ID]: { kind: "sourceGone", archiveMatchesDigest: true },
  [DEMO_ARCHIVE_SOURCE_GONE_UNSOUND_ID]: { kind: "sourceGone", archiveMatchesDigest: false },
  [DEMO_ARCHIVE_DAMAGED_ID]: {
    kind: "archiveDamaged",
    recordedDigest: "1a2b3c4d5e6f70899fedcba012345678",
    currentDigest: "90a1b2c3d4e5f60712345678abcdefab",
  },
};

/** Mirrors `ArchiveIntegrity::copy_is_sound` / `::rebuildable`
 *  (`ct_domain::model::archive`) so the demo booleans agree with the rule
 *  the domain actually enforces, rather than being typed in by hand beside
 *  it -- the same discipline `demoStepAt` uses for the residual step rule. */
function deriveArchiveVerification(integrity: ArchiveIntegritySummary): ArchiveVerification {
  const copyIsSound =
    integrity.kind === "archiveDamaged"
      ? false
      : integrity.kind === "sourceGone"
        ? integrity.archiveMatchesDigest
        : true;
  const rebuildable = integrity.kind === "sourceChanged" || integrity.kind === "archiveDamaged";
  return { integrity, copyIsSound, rebuildable };
}

/** Everything this demo pretends the archive holds, most recently archived
 *  first like the real listing. Two entries reuse a live demo session's id
 *  (the ordinary case: the source is still on disk) and three name sessions
 *  that appear nowhere in `demoSessions` at all -- the archive is the only
 *  place left holding them, which is what `sourceGone` and `archiveDamaged`
 *  mean. */
export const demoArchiveHolding: ArchiveHolding = {
  root: DEMO_ARCHIVE_ROOT,
  entries: [
    {
      id: DEMO_ARCHIVE_DAMAGED_ID,
      agent: "codex",
      project: "C:\\work\\bit-rot-check",
      archivedAt: ago(300),
      redaction: "redacted",
      records: 12,
      sourceBytes: 40_000,
      archivedBytes: 40_000,
      redactedRecords: 0,
      redactedValues: 0,
      differsFromSource: false,
    },
    {
      id: DEMO_ARCHIVE_SOURCE_GONE_UNSOUND_ID,
      agent: "claude-code",
      project: "C:\\work\\retired-service",
      archivedAt: ago(1_400),
      // Raw is the explicit, unusual opt-in -- the one demo row that carries
      // it, so the "not the default" copy has something concrete beside it.
      redaction: "raw",
      records: 58,
      sourceBytes: 98_000,
      archivedBytes: 98_000,
      redactedRecords: 0,
      redactedValues: 0,
      differsFromSource: false,
    },
    {
      id: DEMO_ARCHIVE_SOURCE_GONE_SOUND_ID,
      agent: "codex",
      project: "C:\\work\\deprecated-tool",
      archivedAt: ago(900),
      redaction: "redacted",
      records: 96,
      sourceBytes: 210_000,
      archivedBytes: 210_340,
      redactedRecords: 2,
      redactedValues: 3,
      // The row this flag exists for: the copy is not byte-identical to a
      // source that no longer exists to compare it against.
      differsFromSource: true,
    },
    {
      id: DEMO_ARCHIVE_SOURCE_CHANGED_ID,
      agent: "codex",
      project: "C:\\work\\semantic-search",
      archivedAt: ago(50),
      redaction: "redacted",
      records: 210,
      sourceBytes: 640_000,
      archivedBytes: 640_000,
      redactedRecords: 0,
      redactedValues: 0,
      differsFromSource: false,
    },
    {
      id: "0198fce2e48a7b12",
      agent: "codex",
      project: "C:\\work\\ContextTrace",
      archivedAt: ago(4),
      redaction: "redacted",
      records: 842,
      sourceBytes: 3_829_760,
      archivedBytes: 3_829_760,
      redactedRecords: 0,
      redactedValues: 0,
      differsFromSource: false,
    },
  ],
};

/** Verifying one demo archived session. Any id not named in
 *  `DEMO_ARCHIVE_INTEGRITY` reports `intact` -- the ordinary case for a copy
 *  nothing has checked yet, not a refusal. */
export function demoArchiveVerification(_agent: Agent, id: string): ArchiveVerification {
  return deriveArchiveVerification(DEMO_ARCHIVE_INTEGRITY[id] ?? { kind: "intact" });
}

const growthValues = [
  12_840, 15_320, 18_010, 22_870, 27_430, 31_220, 37_980, 41_500,
  46_860, 52_340, 58_900, 64_120, 69_400, 74_890, 80_420, 85_170,
  28_600, 34_800, 42_300, 49_700, 55_100, 63_400, 70_800, 76_200,
  81_900, 88_400, 94_200, 99_800, 105_600, 111_340, 116_980, 121_760,
];

const growth: GrowthPoint[] = growthValues.map((promptTokens, index) => ({
  turn: index + 1,
  promptTokens,
  compaction:
    index === 16
      ? { turn: 17, reclaimed: 61_200, lineNo: DEMO_COMPACTION_LINE_NO }
      : null,
}));

export function demoDetail(id: string): SessionDetail {
  const session = demoSessions.find((candidate) => candidate.id === id) ?? demoSessions[0];
  return {
    session,
    model: session.agent === "codex" ? "gpt-5.4" : "claude-opus-4.5",
    agentVersion: "0.1-preview",
    gitBranch: "main",
    turnCount: growth.length,
    eventCount: 1_842,
    totalOutputTokens: 76_320,
    peakTurn: 32,
    peakPromptTokens: 121_760,
    contextWindow: 200_000,
    fidelity: 1,
    unrecognisedEvents: 0,
    unplacedCompactions: 0,
    growth,
  };
}

export function demoContext(turn = 32): ContextDetail {
  const promptTokens = growth.find((point) => point.turn === turn)?.promptTokens ?? 121_760;
  const scale = promptTokens / 121_760;
  const sized = (tokens: number) => Math.round(tokens * scale);
  const categories = [
    ["tool-outputs", "Tool outputs", 43_720, 18, "derived"],
    ["assistant-messages", "Assistant messages", 22_840, 21, "derived"],
    ["file-contents", "File contents", 17_260, 9, "derived"],
    ["user-messages", "User messages", 13_920, 20, "observed"],
    ["system-instructions", "System instructions", 10_180, 3, "observed"],
    ["reasoning", "Reasoning", 8_040, 12, "derived"],
    ["unattributed", "Unattributed", 5_800, 1, "estimated"],
  ] as const;
  const total = categories.reduce((sum, [, , tokens]) => sum + sized(tokens), 0);

  return {
    turn,
    model: "gpt-5.4",
    totalTokens: total,
    residualTokens: sized(5_800),
    residualIsMeaningful: true,
    contextWindow: 200_000,
    utilisation: total / 200_000,
    calibrationScale: 0.94,
    categories: categories.map(([category, label, tokens, itemCount, confidence]) => ({
      category,
      label,
      tokens: sized(tokens),
      share: sized(tokens) / total,
      // Item counts grow with the turn, like the tokens do. Holding them fixed
      // made every turn-to-turn comparison report "0 items" changed while the
      // token columns moved, which is not a shape a real session can produce.
      itemCount: Math.max(1, Math.round(itemCount * scale)),
      confidence,
    })),
    contributors: [
      ["tool: shell_command → test output", "Tool outputs", "tool: shell_command", 18_430, "derived"],
      ["crates/ct-adapters/src/codex/reconstruct.rs", "File contents", "file read", 12_890, "observed"],
      ["Compilation and linker diagnostics", "Tool outputs", "tool: cargo", 9_720, "derived"],
      ["Repository instructions", "Repository instructions", "instruction file AGENTS.md", 7_340, "observed"],
      ["Prior assistant implementation", "Assistant messages", "model output", 6_810, "derived"],
      ["Tauri configuration reference", "File contents", "file read", 5_420, "observed"],
      ["Current implementation request", "Current prompt", "user prompt", 3_980, "observed"],
      ["Workspace dependency graph", "Tool outputs", "tool: cargo metadata", 2_760, "derived"],
    ].map(([label, category, source, tokens, confidence], index) => ({
      id: `demo-${index}`,
      label: String(label),
      category: String(category),
      source: String(source),
      tokens: sized(Number(tokens)),
      share: sized(Number(tokens)) / total,
      confidence: confidence as "observed" | "derived" | "estimated",
    })),
  };
}

export function demoDoctor(turn = 32): DoctorReport {
  return {
    turn,
    duplicateGroups: 3,
    repeatedTokens: 18_240,
    duplicates: [
      {
        copies: 3,
        totalTokens: 27_360,
        repeatedTokens: 18_240,
        share: 0.15,
        confidence: "derived",
        items: [
          { label: "Repeated test output", source: "tool: shell_command", tokens: 9_120 },
          { label: "Repeated test output", source: "tool: shell_command", tokens: 9_120 },
          { label: "Repeated test output", source: "tool: shell_command", tokens: 9_120 },
        ],
      },
    ],
    lowEntropyItems: 2,
    wasteScoreTokens: 13_480,
    lowEntropy: [
      {
        label: "Compilation and linker diagnostics",
        source: "tool: cargo",
        tokens: 17_200,
        compressionRatio: 0.22,
        wasteScoreTokens: 13_416,
        share: 0.14,
        confidence: "derived",
      },
    ],
    secretFindings: 0,
    secretOccurrences: 0,
    scannedRecords: 196,
    unreadableRecords: 0,
    unmeasuredItems: 12,
    secrets: [],
  };
}

export function demoLifecycle(item: string): LifecycleReport {
  const contributor = demoContext().contributors.find((candidate) => candidate.id === item);
  return {
    id: item,
    label: contributor?.label ?? "Context item",
    category: contributor?.category ?? "Unknown",
    source: contributor?.source ?? "unknown",
    firstPresent: 9,
    lastPresent: 32,
    turnsPresent: 24,
    runs: [{ from: 9, to: 32, turns: 24 }],
    departure: null,
    stillPresent: true,
    unknownTurns: [],
    scannedTurns: 32,
    otherThreadTurns: 0,
    lastScannedTurn: 32,
    recordedFirstSeen: 8,
    firstSeenDisagrees: true,
  };
}

/**
 * Demonstrates all three shapes of `CompactionDiff` a real session can
 * return, split by agent -- Codex records replacement history and Claude
 * Code never does, so the refusal is not a fabricated edge case but the
 * ordinary answer for one whole agent. Shaped to match the committed Codex
 * fixture's one compaction (4 dropped, 1 preserved, 1 replacement-only; see
 * BACKLOG.md CT-047) so a developer running `npm run dev` sees the same mix
 * the acceptance run checks against real data.
 */
export function demoCompactionDiff(agent: Agent, lineNo: number): CompactionDiff {
  if (agent === "claude-code") {
    return {
      status: "unsupported",
      detail:
        "compaction item diff for claude-code: this agent does not record a literal replacement history",
    };
  }
  return {
    status: "available",
    turn: 17,
    lineNo,
    items: [
      {
        itemType: "message",
        role: "user",
        disposition: { kind: "dropped", historyIndex: 0 },
        normalizedJsonBytes: 812,
        textTokens: 210,
        confidence: "derived",
      },
      {
        itemType: "function_call",
        role: null,
        disposition: { kind: "dropped", historyIndex: 1 },
        normalizedJsonBytes: 640,
        textTokens: 140,
        confidence: "derived",
      },
      {
        itemType: "function_call_output",
        role: null,
        disposition: { kind: "dropped", historyIndex: 2 },
        normalizedJsonBytes: 15_420,
        textTokens: null,
        confidence: "derived",
      },
      {
        itemType: "reasoning",
        role: null,
        disposition: { kind: "dropped", historyIndex: 3 },
        normalizedJsonBytes: 3_960,
        textTokens: 940,
        confidence: "derived",
      },
      {
        itemType: "message",
        role: "user",
        disposition: { kind: "preserved", historyIndex: 4, replacementIndex: 0 },
        normalizedJsonBytes: 1_180,
        textTokens: 260,
        confidence: "derived",
      },
      {
        itemType: "message",
        role: "assistant",
        disposition: { kind: "addedByReplacement", replacementIndex: 1 },
        normalizedJsonBytes: 4_320,
        textTokens: null,
        confidence: "derived",
      },
    ],
  };
}

/**
 * Fitted characters-per-token ratios for the demo Claude Code sessions,
 * named individually rather than derived from a hash of the id.
 *
 * A hash can collide two different sessions onto the same ratio, which would
 * make a cross-session comparison between them report `identical` -- the
 * exact defect a hash-based stand-in would reproduce in demo mode. Naming
 * the three real Claude Code demo sessions here, each with its own figure,
 * is what `ct_runtime::calibrate_session` actually does: one fit per
 * session, from that session's own turns.
 */
const DEMO_CLAUDE_RATIOS: Record<string, number> = {
  "a30cb9e1-f9f4-4a37": 2.05,
  "f485150f-0982-4876": 2.42,
  "b71d4c08-2e55-41aa": 2.3,
};

/** The instrument one demo session would report, mirroring
 *  `ct_application::Instrument` and how `commands.rs`'s `turn_diff` picks
 *  one: a fitted ratio for Claude Code, the session's own tokenizer name for
 *  Codex. Codex's `chars_per_token` is `null` -- it has nothing to fit --
 *  which is what lets two different Codex sessions still compare as
 *  `identical` below, the same as the real backend. */
function demoInstrument(agent: Agent, id: string): { name: string; charsPerToken: number | null } {
  if (agent === "codex") return { name: "o200k_base", charsPerToken: null };
  const ratio = DEMO_CLAUDE_RATIOS[id] ?? 2.2;
  return { name: `heuristic:chars/${ratio.toFixed(1)}`, charsPerToken: ratio };
}

/**
 * Mirrors `Comparability::of` (`ct_application::diff`) exactly, including its
 * reason text, so a demo cross-session comparison is reachable for *any* two
 * demo sessions a caller names -- not one hand-picked pair -- and reports the
 * same kind a real backend would for the same instrument combination.
 */
function demoComparability(left: TurnTarget, right: TurnTarget): Comparability {
  const agentsDiffer = left.agent !== right.agent;
  const l = demoInstrument(left.agent, left.id);
  const r = demoInstrument(right.agent, right.id);

  if (l.charsPerToken != null && r.charsPerToken != null) {
    if (l.charsPerToken === r.charsPerToken && l.name === r.name) {
      return { kind: "identical", estimator: l.name };
    }
    return { kind: "skewed", left: l.name, right: r.name, skew: Math.abs(l.charsPerToken / r.charsPerToken - 1) };
  }
  if (l.charsPerToken == null && r.charsPerToken == null && l.name === r.name) {
    return { kind: "identical", estimator: l.name };
  }
  return {
    kind: "incomparable",
    left: l.name,
    right: r.name,
    reason: agentsDiffer
      ? "one side is measured with a tokenizer and the other estimated from a ratio, and the " +
        "two agents do not log the same things -- Codex records its system prompt, so the " +
        "residuals are not the same quantity"
      : "the two sides were sized by different kinds of instrument, and no factor relates their scales",
  };
}

/**
 * A comparison of two turns -- of one demo session, as before, or of two
 * different ones now that the panel can ask for that.
 *
 * Category and tool figures are still read from `demoContext`, so the deltas
 * agree with what the composition panel shows for each turn rather than
 * being a second, independently invented set of numbers. What changed is
 * `comparability`: it is computed from the two sides' identities via
 * `demoComparability` instead of asserted as `identical`, so `skewed` and
 * `incomparable` are genuinely reachable here -- pick two different Claude
 * Code demo sessions for a skew, or a Codex and a Claude Code session for an
 * incomparable pair -- exactly as choosing them would produce on a real
 * backend.
 */
export function demoTurnDiff(left: TurnTarget, right: TurnTarget): TurnDiff {
  const leftContext = demoContext(left.turn);
  const rightContext = demoContext(right.turn);
  const byCategory = (detail: ContextDetail, label: string) =>
    detail.categories.find((category) => category.label === label);

  const comparability = demoComparability(left, right);
  // `Comparability::of` never widens the bound for skew; `identical` bounds
  // nothing away (0) and `incomparable` states no bound at all (null) --
  // mirrored here rather than read off `tokensComparable` in the UI, so the
  // fixture is honest about what a real backend would attach to each row.
  const skew =
    comparability.kind === "identical" ? 0 : comparability.kind === "skewed" ? comparability.skew : null;

  const labels = [...new Set(leftContext.categories.concat(rightContext.categories).map((c) => c.label))];
  const categories = labels
    .map((label) => {
      const l = byCategory(leftContext, label);
      const r = byCategory(rightContext, label);
      const leftTokens = l?.tokens ?? 0;
      const rightTokens = r?.tokens ?? 0;
      const bound = skew == null ? null : Math.ceil(Math.max(leftTokens, rightTokens) * skew);
      return {
        category: label,
        left: leftTokens,
        right: rightTokens,
        delta: rightTokens - leftTokens,
        leftItems: l?.itemCount ?? 0,
        rightItems: r?.itemCount ?? 0,
        itemDelta: (r?.itemCount ?? 0) - (l?.itemCount ?? 0),
        instrumentBound: bound,
        meaningful: bound != null && Math.abs(rightTokens - leftTokens) > bound,
      };
    })
    .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta));

  const side = (target: TurnTarget, detail: ContextDetail) => ({
    id: target.id,
    agent: target.agent,
    turn: detail.turn,
    totalTokens: detail.totalTokens,
    items: detail.categories.reduce((sum, category) => sum + category.itemCount, 0),
    residual: detail.residualTokens,
    confidence: "derived" as const,
  });

  return {
    left: side(left, leftContext),
    right: side(right, rightContext),
    comparability,
    promptDelta: rightContext.totalTokens - leftContext.totalTokens,
    totalsAreObserved: true,
    categories,
    tools: [
      ["shell_command", 11, 18_420],
      ["read_file", 5, 7_260],
    ].map(([tool, calls, tokens]) => {
      // Derived from each side's own turn rather than fixed, so a comparison
      // never claims a later turn made fewer calls than an earlier one.
      const at = (detail: ContextDetail) => {
        const share = detail.totalTokens / 121_760;
        return {
          calls: Math.max(1, Math.round((calls as number) * share)),
          tokens: Math.round((tokens as number) * share),
        };
      };
      const l = at(leftContext);
      const r = at(rightContext);
      const bound = skew == null ? null : Math.ceil(Math.max(l.tokens, r.tokens) * skew);
      return {
        tool: String(tool),
        leftCalls: l.calls,
        rightCalls: r.calls,
        leftTokens: l.tokens,
        rightTokens: r.tokens,
        callDelta: r.calls - l.calls,
        tokenDelta: r.tokens - l.tokens,
        instrumentBound: bound,
        meaningful: bound != null && Math.abs(r.tokens - l.tokens) > bound,
      };
    }),
  };
}

export function demoInstructionFiles(id: string): InstructionFileReport {
  return {
    sessionId: id,
    projectRoot: "C:\\work\\ContextTrace",
    comparisons: [
      {
        path: "C:\\work\\ContextTrace\\AGENTS.md",
        turn: 1,
        line: 3,
        status: "matching",
        recordedDigest: "a".repeat(64),
        currentDigest: "a".repeat(64),
        recordedChars: 1280,
        currentChars: 1280,
        comparisonBasis: "SHA-256 of the recorded attachment payload versus SHA-256 of current file bytes",
        detail: "recorded attachment content fingerprint equals current file bytes",
      },
    ],
    refusalCount: 0,
  };
}

export function demoTemporalGhost(leftTurn: number, rightTurn: number): TemporalGhost {
  const left = demoContext(leftTurn);
  const right = demoContext(rightTurn);
  const item = (candidate: typeof left.contributors[number], side: "left" | "right") => ({
    id: candidate.id,
    label: candidate.label,
    category: candidate.category,
    source: candidate.source,
    leftTokens: side === "left" ? candidate.tokens : null,
    rightTokens: side === "right" ? candidate.tokens : null,
    tokenDelta: null,
    meaningfulTokenDelta: false,
    confidence: candidate.confidence,
  });
  return {
    status: "available",
    leftTurn,
    rightTurn,
    comparability: { kind: "identical", estimator: "demo" },
    gained: [item(right.contributors[1], "right")],
    retained: [item(left.contributors[0], "left")],
    removed: [item(left.contributors[2], "left")],
    assumptions: [
      "Demo content is illustrative and is not read from a local session.",
      "Item identity is the adapter's stable context-item id; it is not a text diff.",
    ],
  };
}

export function demoCost(id: string, forecastTurns: number): CostReport {
  const category = { name: "input", tokens: 48_000, cost: 240_000, confidence: "estimated" };
  return {
    sessionId: id,
    pricingVersion: "demo",
    pricingSource: "synthetic demo table",
    warning: "Demo data; no local pricing file was read.",
    categories: [category],
    total: category.cost,
    turns: [],
    unpriced: [],
    forecast: forecastTurns > 0
      ? {
          additionalTurns: forecastTurns,
          averageTokensPerTurn: [category],
          projectedAdditional: category.cost * forecastTurns,
          projectedTotal: category.cost * (forecastTurns + 1),
          assumptions: ["Demo forecast uses one synthetic priced turn."],
        }
      : null,
  };
}

/**
 * The hidden constant this demo session is pretending to carry, turn by turn.
 *
 * Flat, then two sustained rises: the first at the compaction, where a rewritten
 * prompt plausibly changes what reconstruction can see, and the second with no
 * event in the log beside it — the shape that means a tool registered or an MCP
 * server connected. Both clear the 5,000-token step threshold, so the panel's
 * step markers are produced by the series rather than asserted alongside it.
 */
function demoOverheadAt(turn: number): number {
  const level = turn < 18 ? 9_400 : turn < 24 ? 15_200 : 22_700;
  // Plus drift and wobble, because a perfectly flat remainder is not a shape a
  // real session produces — and the panel's own caption says so. One fitted
  // ratio cannot describe a session that starts as prose and ends dominated by
  // tool output, so the remainder creeps upward and jitters, and the step
  // detector's whole job is to survive that. A flat demo line would show a
  // detector with nothing to survive.
  const drift = turn * 55;
  const wobble = (((turn * 37) % 11) - 5) * 90;
  return level + drift + wobble;
}

/** Median of the values, for recovering a step's levels from the series. */
function medianOf(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 1
    ? sorted[middle]
    : Math.round((sorted[middle - 1] + sorted[middle]) / 2);
}

/**
 * Recover each demo step's levels from the demo series, by the rule the backend
 * uses: the median of the five measured turns either side.
 *
 * Typing the levels in by hand would let the markers claim a move the line does
 * not make — which is the same defect as fabricating the fit, one layer down.
 * Turns whose remainder is unknown are excluded from the windows here too,
 * because they are excluded there.
 *
 * The five is `ct_application::STEP_WINDOW`. This is the one place that number
 * is duplicated in TypeScript; if it moves there, these demo markers go quietly
 * wrong and only looking at the chart would show it.
 */
function demoStepAt(points: ResidualPoint[], turn: number, nearCompaction: boolean): ResidualStep {
  const known = points.filter(
    (point): point is ResidualPoint & { unlogged: number } => point.unlogged != null,
  );
  const index = known.findIndex((point) => point.turn === turn);
  const from = medianOf(known.slice(index - 5, index).map((point) => point.unlogged));
  const to = medianOf(known.slice(index, index + 5).map((point) => point.unlogged));
  return { turn, from, to, growth: to - from, nearCompaction };
}

/** The one demo turn whose reconstruction exceeds its own prompt. Placed
 *  straight after the compaction, where a rebuilt ancestor chain is the
 *  documented cause of over-counting. */
const DEMO_OVER_COUNTED_TURN = 17;

/**
 * A fabricated unlogged-context measurement.
 *
 * Every point is built as `accounted = promptTokens - unlogged`, so the two
 * columns and the prompt agree by construction rather than by three lists
 * happening to have been typed consistently. The over-counted turn is the
 * exception and is the only one: its accounted figure exceeds the prompt, which
 * is exactly why its remainder is `null` and not a number.
 */
function demoResidualSeries(): ResidualPoint[] {
  return growth.flatMap<ResidualPoint>((point) => {
    if (point.promptTokens == null) return [];
    const items = Math.max(4, Math.round((point.promptTokens / 121_760) * 84));
    if (point.turn === DEMO_OVER_COUNTED_TURN) {
      return [
        {
          turn: point.turn,
          promptTokens: point.promptTokens,
          accounted: point.promptTokens + 1_500,
          unlogged: null,
          items,
        },
      ];
    }
    const unlogged = demoOverheadAt(point.turn);
    return [
      {
        turn: point.turn,
        promptTokens: point.promptTokens,
        accounted: point.promptTokens - unlogged,
        unlogged,
        items,
      },
    ];
  });
}

/**
 * Which residual state a demo session is in, keyed by identity.
 *
 * All four are reachable: Codex sessions refuse because no ratio is fitted for
 * that agent at all, the short session refuses for want of growth, one Claude
 * Code session over-counts throughout, and one carries the full series. A demo
 * that only rendered the successful case would show the panel's easy state and
 * hide the three it exists to get right.
 */
export function demoResidual(agent: Agent, id: string): ResidualReport {
  if (agent === "codex") return { kind: "agentNotFitted", agent };
  if (id === RESIDUAL_SHORT_SESSION) return { kind: "insufficientGrowth", turnsWithUsage: 3 };
  if (id === RESIDUAL_OVER_COUNTED_SESSION) {
    return {
      kind: "overCounted",
      charsPerToken: 2.84,
      pairsUsed: 19,
      dispersion: 1.62,
      turnsMeasured: 41,
    };
  }

  const points = demoResidualSeries();
  const measured = points
    .map((point) => point.unlogged)
    .filter((unlogged): unlogged is number => unlogged != null);

  return {
    kind: "fitted",
    charsPerToken: 3.42,
    pairsUsed: 24,
    dispersion: 1.19,
    // Read off the series rather than typed in beside it, so the session's
    // stated constant is the one its own turns carry.
    unloggedOverhead: medianOf(measured),
    turnsMeasured: points.length,
    overCountedTurns: points.filter((point) => point.unlogged == null).length,
    stepThreshold: 5_000,
    promptConfidence: "observed",
    remainderConfidence: "derived",
    points,
    steps: [
      // Turn 18 sits one turn after the demo compaction, so the panel has to
      // decline to narrate it; turn 24 has nothing beside it in the log.
      demoStepAt(points, 18, true),
      demoStepAt(points, 24, false),
    ],
  };
}
