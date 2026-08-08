import type {
  Agent,
  CompactionDiff,
  ContextDetail,
  DoctorReport,
  GrowthPoint,
  LifecycleReport,
  ResidualPoint,
  ResidualReport,
  ResidualStep,
  SessionDetail,
  SessionSummary,
  StartupSummary,
  ThreadRole,
  TurnDiff,
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

/** The demo sessions each residual state is attached to, so all four are
 *  reachable in the browser without a local corpus. */
const RESIDUAL_FITTED_SESSION = "a30cb9e1-f9f4-4a37";
const RESIDUAL_OVER_COUNTED_SESSION = "f485150f-0982-4876";
const RESIDUAL_SHORT_SESSION = "b71d4c08-2e55-41aa";

export const demoStartup: StartupSummary = {
  roots: [
    { agent: "codex", paths: ["C:\\Users\\demo\\.codex\\sessions"] },
    { agent: "claude-code", paths: ["C:\\Users\\demo\\.claude\\projects"] },
  ],
  warnings: [],
};

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
 * A comparison of two turns of the demonstration session.
 *
 * Derived from the same category table and growth series `demoContext` uses,
 * so the deltas here agree with what the composition panel shows for those two
 * turns rather than being a second, independently invented set of numbers.
 *
 * Comparability is `identical` because both sides come from one session, and a
 * session is calibrated once — which is what the real backend reports on this
 * path too. Fabricating a `skewed` or `incomparable` case here would put a
 * state on screen that the app cannot actually produce.
 */
export function demoTurnDiff(leftTurn: number, rightTurn: number): TurnDiff {
  const left = demoContext(leftTurn);
  const right = demoContext(rightTurn);
  const byCategory = (detail: ContextDetail, label: string) =>
    detail.categories.find((category) => category.label === label);

  const labels = [...new Set(left.categories.concat(right.categories).map((c) => c.label))];
  const categories = labels
    .map((label) => {
      const l = byCategory(left, label);
      const r = byCategory(right, label);
      const leftTokens = l?.tokens ?? 0;
      const rightTokens = r?.tokens ?? 0;
      return {
        category: label,
        left: leftTokens,
        right: rightTokens,
        delta: rightTokens - leftTokens,
        leftItems: l?.itemCount ?? 0,
        rightItems: r?.itemCount ?? 0,
        itemDelta: (r?.itemCount ?? 0) - (l?.itemCount ?? 0),
        // One instrument on both sides, so nothing but content can move a row.
        instrumentBound: 0,
        meaningful: rightTokens !== leftTokens,
      };
    })
    .sort((a, b) => Math.abs(b.delta) - Math.abs(a.delta));

  const side = (detail: ContextDetail) => ({
    turn: detail.turn,
    totalTokens: detail.totalTokens,
    items: detail.categories.reduce((sum, category) => sum + category.itemCount, 0),
    residual: detail.residualTokens,
    confidence: "derived" as const,
  });

  return {
    left: side(left),
    right: side(right),
    comparability: { kind: "identical", estimator: "o200k_base" },
    promptDelta: right.totalTokens - left.totalTokens,
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
      const l = at(left);
      const r = at(right);
      return {
        tool: String(tool),
        leftCalls: l.calls,
        rightCalls: r.calls,
        leftTokens: l.tokens,
        rightTokens: r.tokens,
        callDelta: r.calls - l.calls,
        tokenDelta: r.tokens - l.tokens,
        instrumentBound: 0,
        meaningful: r.tokens !== l.tokens,
      };
    }),
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
