import type {
  ContextDetail,
  DoctorReport,
  GrowthPoint,
  SessionDetail,
  SessionSummary,
  StartupSummary,
} from "./types";

const now = Date.now();
const ago = (hours: number) => new Date(now - hours * 3_600_000).toISOString();

export const demoSessions: SessionSummary[] = [
  {
    id: "0198fce2e48a7b12",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\contexttrace.jsonl",
    sizeBytes: 3_829_760,
    project: "C:\\work\\ContextTrace",
    startedAt: ago(5),
    lastActivity: ago(0.4),
  },
  {
    id: "a30cb9e1-f9f4-4a37",
    agent: "claude-code",
    path: "C:\\Users\\demo\\.claude\\projects\\atlas\\session.jsonl",
    sizeBytes: 1_677_721,
    project: "C:\\work\\atlas-dashboard",
    startedAt: ago(30),
    lastActivity: ago(23),
  },
  {
    id: "0198fb914e330a81",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\search.jsonl",
    sizeBytes: 812_413,
    project: "C:\\work\\semantic-search",
    startedAt: ago(51),
    lastActivity: ago(47),
  },
  {
    id: "f485150f-0982-4876",
    agent: "claude-code",
    path: "C:\\Users\\demo\\.claude\\projects\\payments\\session.jsonl",
    sizeBytes: 7_130_317,
    project: "C:\\work\\payment-service",
    startedAt: ago(76),
    lastActivity: ago(70),
  },
  {
    id: "0198f420c9740dac",
    agent: "codex",
    path: "C:\\Users\\demo\\.codex\\sessions\\compiler.jsonl",
    sizeBytes: 2_075_648,
    project: "C:\\work\\compiler-lab",
    startedAt: ago(110),
    lastActivity: ago(99),
  },
];

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
      ? { turn: 17, reclaimed: 61_200 }
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
      itemCount,
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
    secrets: [],
  };
}
