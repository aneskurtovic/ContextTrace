import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { getContext, searchSessions } from "./api";

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  invoke.mockReset();
});

describe("desktop IPC response validation", () => {
  it("rejects malformed context payloads with a stable, actionable error", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({ turn: 3, totalTokens: "not-a-number", categories: [] });

    await expect(getContext("fixture-session", 3)).rejects.toThrow(
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
    });
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
});
