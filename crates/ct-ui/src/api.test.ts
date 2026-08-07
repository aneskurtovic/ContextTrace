import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { getContext, getLifecycle, runDoctor, searchSessions } from "./api";

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
    });

    await expect(runDoctor("codex", "session-1", 7)).resolves.toMatchObject({ turn: 7 });
    expect(invoke).toHaveBeenCalledWith("run_doctor", {
      id: "session-1",
      agent: "codex",
      turn: 7,
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
});
