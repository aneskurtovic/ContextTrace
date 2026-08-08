import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  getCompactionDiff,
  getContext,
  getLifecycle,
  inspectSession,
  runDoctor,
  searchSessions,
} from "./api";
import { demoCompactionDiff, demoDetail, demoDoctor, demoSessions } from "./demo";

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  invoke.mockReset();
});

describe("desktop IPC response validation", () => {
  it("rejects malformed context payloads with a stable, actionable error", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({ turn: 3, totalTokens: "not-a-number", categories: [] });

    await expect(getContext("codex", "fixture-session", 3)).rejects.toThrow(
      "ContextTrace received an invalid response from context reconstruction. Refresh and try again.",
    );
  });

  it("validates and forwards the paged session search contract", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      sessions: [],
      total: 501,
      offset: 500,
      hasMore: false,
    });

    await expect(searchSessions("codex", "older", 500, 200)).resolves.toEqual({
      sessions: [],
      total: 501,
      offset: 500,
      hasMore: false,
    });
    expect(invoke).toHaveBeenCalledWith("search_sessions", {
      agent: "codex",
      query: "older",
      offset: 500,
      limit: 200,
      refresh: false,
    });
  });

  it("only asks the backend to discard its caches on an explicit refresh", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const page = { sessions: [], total: 0, offset: 0, hasMore: false };
    invoke.mockResolvedValue(page);

    await searchSessions("codex", "query", 200, 200);
    expect(invoke).toHaveBeenLastCalledWith(
      "search_sessions",
      expect.objectContaining({ refresh: false }),
    );

    await searchSessions("codex", "query", 0, 200, true);
    expect(invoke).toHaveBeenLastCalledWith(
      "search_sessions",
      expect.objectContaining({ refresh: true }),
    );
  });

  it("validates a session's thread role, root and subagent alike", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const root = { ...demoSessions[0], threadRole: { kind: "root", parent: null } };
    const subagent = {
      ...demoSessions[0],
      id: "subagent-id",
      threadRole: { kind: "subagent", parent: demoSessions[0].id },
    };
    invoke.mockResolvedValue({
      sessions: [root, subagent],
      total: 2,
      offset: 0,
      hasMore: false,
    });

    await expect(searchSessions()).resolves.toMatchObject({
      sessions: [root, subagent],
    });
  });

  it("rejects a session whose thread role pairs a subagent kind with no parent", async () => {
    // The Rust `ThreadRole` makes this combination unrepresentable; the
    // frontend validator has to reject it too, or a malformed IPC payload
    // would slip past the boundary the type system enforces on the other
    // side.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      sessions: [{ ...demoSessions[0], threadRole: { kind: "subagent", parent: null } }],
      total: 1,
      offset: 0,
      hasMore: false,
    });

    await expect(searchSessions()).rejects.toThrow(
      "ContextTrace received an invalid response from session search. Refresh and try again.",
    );
  });

  it("rejects a session reporting a root with a parent still attached", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      sessions: [{ ...demoSessions[0], threadRole: { kind: "root", parent: "stray-parent" } }],
      total: 1,
      offset: 0,
      hasMore: false,
    });

    await expect(searchSessions()).rejects.toThrow(
      "ContextTrace received an invalid response from session search. Refresh and try again.",
    );
  });

  it("holds the demo session catalog to the same contract as the real backend", async () => {
    // Every other test in this suite stubs `__TAURI_INTERNALS__` and only
    // exercises the IPC branch, so nothing else would notice `demo.ts`
    // drifting away from what `search_sessions` actually returns. Feeding the
    // demo catalog through the validated IPC path is what makes a field wired
    // into the Rust struct and `types.ts` but forgotten in `demo.ts` (or vice
    // versa) fail here instead of only at runtime in the unpackaged app.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      sessions: demoSessions,
      total: demoSessions.length,
      offset: 0,
      hasMore: false,
    });

    await expect(searchSessions()).resolves.toMatchObject({ total: demoSessions.length });
    expect(demoSessions.some((session) => session.threadRole.kind === "subagent")).toBe(true);
  });

  it("rejects malformed paged session responses", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      sessions: [],
      total: "501",
      offset: 0,
      hasMore: false,
    });

    await expect(searchSessions()).rejects.toThrow(
      "ContextTrace received an invalid response from session search. Refresh and try again.",
    );
  });

  it("validates and forwards the Context Doctor contract", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      turn: 7,
      duplicateGroups: 0,
      repeatedTokens: 0,
      duplicates: [],
      lowEntropyItems: 0,
      wasteScoreTokens: 0,
      lowEntropy: [],
      secretFindings: 0,
      secretOccurrences: 0,
      scannedRecords: 12,
      unreadableRecords: 0,
      secrets: [],
      unmeasuredItems: 4,
    });

    await expect(runDoctor("codex", "session-1", 7)).resolves.toMatchObject({
      turn: 7,
      unmeasuredItems: 4,
    });
    expect(invoke).toHaveBeenCalledWith("run_doctor", {
      id: "session-1",
      agent: "codex",
      turn: 7,
    });
  });

  it("rejects a Context Doctor payload missing the unmeasured-item count", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      turn: 7,
      duplicateGroups: 0,
      repeatedTokens: 0,
      duplicates: [],
      lowEntropyItems: 0,
      wasteScoreTokens: 0,
      lowEntropy: [],
      secretFindings: 0,
      secretOccurrences: 0,
      scannedRecords: 12,
      unreadableRecords: 0,
      secrets: [],
      // unmeasuredItems intentionally omitted -- CT-059 requires the doctor
      // report to always carry this count, even when it is zero, because a
      // structured consumer cannot tell "zero unmeasured" from "field never
      // wired up" any other way.
    });

    await expect(runDoctor("codex", "session-1", 7)).rejects.toThrow(
      "invalid response from Context Doctor",
    );
  });

  it("holds the demo doctor payload to the same contract as the real backend", async () => {
    // The demo path returns `demoDoctor()` without validating it, so nothing
    // else in this suite would notice demo data drifting away from the
    // contract the backend is held to -- every other test stubs
    // `__TAURI_INTERNALS__` and therefore only ever exercises the IPC branch.
    // Feeding the demo payload through that branch is what makes the drift
    // detectable: a field added to `DoctorReport` and wired into the Rust
    // struct but forgotten in `demo.ts` fails here.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue(demoDoctor(7));

    await expect(runDoctor("codex", "session-1", 7)).resolves.toMatchObject({
      unmeasuredItems: expect.any(Number),
    });
  });

  it("rejects malformed Context Doctor findings", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      turn: 7,
      duplicateGroups: 1,
      repeatedTokens: 100,
      duplicates: [{ copies: "two" }],
      lowEntropyItems: 0,
      wasteScoreTokens: 0,
      lowEntropy: [],
      secretFindings: 0,
      secretOccurrences: 0,
      scannedRecords: 12,
      unreadableRecords: 0,
      secrets: [],
      unmeasuredItems: 0,
    });

    await expect(runDoctor("codex", "session-1", 7)).rejects.toThrow(
      "invalid response from Context Doctor",
    );
  });

  it("validates and forwards the item lifecycle contract", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      id: "codex:42",
      label: "Read BACKLOG.md",
      category: "File contents",
      source: "file: BACKLOG.md",
      firstPresent: 3,
      lastPresent: 8,
      turnsPresent: 6,
      runs: [{ from: 3, to: 8, turns: 6 }],
      departure: { kind: "compaction", turn: 9, reclaimed: 20_000 },
      stillPresent: false,
      unknownTurns: [],
      scannedTurns: 10,
      otherThreadTurns: 0,
      lastScannedTurn: 10,
      recordedFirstSeen: 3,
      firstSeenDisagrees: false,
    });

    await expect(getLifecycle("codex", "session-1", "codex:42")).resolves.toMatchObject({
      turnsPresent: 6,
    });
    expect(invoke).toHaveBeenCalledWith("get_lifecycle", {
      id: "session-1",
      agent: "codex",
      item: "codex:42",
    });
  });

  it("holds the demo session detail to the same contract as the real backend", async () => {
    // Nothing else in this suite exercises `inspect_session`, so nothing
    // would notice `demo.ts`'s `lineNo` field on a growth point's compaction
    // drifting away from what the backend actually returns -- exactly the
    // half-wiring CT-047 warns two prior attempts already shipped.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const detail = demoDetail(demoSessions[0].id);
    invoke.mockResolvedValue(detail);

    const resolved = await inspectSession("codex", demoSessions[0].id);
    const compaction = resolved.growth.find((point) => point.compaction)?.compaction;
    expect(compaction?.lineNo).toEqual(expect.any(Number));
  });

  it("validates and forwards an available compaction diff, camelCase all the way into each disposition", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      status: "available",
      turn: 17,
      lineNo: 4821,
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
          itemType: "message",
          role: "user",
          disposition: { kind: "preserved", historyIndex: 4, replacementIndex: 0 },
          normalizedJsonBytes: 1180,
          textTokens: null,
          confidence: "derived",
        },
      ],
    });

    await expect(getCompactionDiff("codex", "session-1", 4821)).resolves.toMatchObject({
      status: "available",
      lineNo: 4821,
    });
    expect(invoke).toHaveBeenCalledWith("get_compaction_diff", {
      id: "session-1",
      agent: "codex",
      lineNo: 4821,
    });
  });

  it("rejects a preserved item missing its replacement index", async () => {
    // The Rust `CompactionItemDisposition::Preserved` variant makes a
    // `historyIndex` without a paired `replacementIndex` unrepresentable;
    // the frontend validator has to reject it too, mirroring the
    // ThreadRole pairing check above.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      status: "available",
      turn: 17,
      lineNo: 4821,
      items: [
        {
          itemType: "message",
          role: "user",
          disposition: { kind: "preserved", historyIndex: 4 },
          normalizedJsonBytes: 1180,
          textTokens: null,
          confidence: "derived",
        },
      ],
    });

    await expect(getCompactionDiff("codex", "session-1", 4821)).rejects.toThrow(
      "invalid response from compaction autopsy",
    );
  });

  it("validates and forwards the unavailable-evidence reason for one compaction", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      status: "unavailable",
      turn: 9,
      lineNo: 120,
      reason: "malformedRawLine",
    });

    await expect(getCompactionDiff("codex", "session-1", 120)).resolves.toEqual({
      status: "unavailable",
      turn: 9,
      lineNo: 120,
      reason: "malformedRawLine",
    });
  });

  it("rejects an unavailable reason outside the known set", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      status: "unavailable",
      turn: 9,
      lineNo: 120,
      reason: "somethingNew",
    });

    await expect(getCompactionDiff("codex", "session-1", 120)).rejects.toThrow(
      "invalid response from compaction autopsy",
    );
  });

  it("validates and forwards the agent-level refusal for an unsupported agent", async () => {
    // Claude Code never records a literal replacement history, so this is
    // the ordinary answer for the whole agent, not an edge case -- it must
    // reach the panel as a typed, explained outcome, not a generic IPC
    // error.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      status: "unsupported",
      detail: "compaction item diff for claude-code: this agent does not record a literal replacement history",
    });

    await expect(getCompactionDiff("claude-code", "session-1", 4821)).resolves.toMatchObject({
      status: "unsupported",
    });
  });

  it("holds the demo compaction diff to the same contract as the real backend, for both agents", async () => {
    // Every other compaction-diff test above stubs `__TAURI_INTERNALS__` and
    // only exercises the IPC branch, so nothing else would notice
    // `demoCompactionDiff` drifting away from the contract the backend is
    // held to.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

    invoke.mockResolvedValue(demoCompactionDiff("codex", 4821));
    await expect(getCompactionDiff("codex", "session-1", 4821)).resolves.toMatchObject({
      status: "available",
    });

    invoke.mockResolvedValue(demoCompactionDiff("claude-code", 4821));
    await expect(getCompactionDiff("claude-code", "session-1", 4821)).resolves.toMatchObject({
      status: "unsupported",
    });
  });
});
