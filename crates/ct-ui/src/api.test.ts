import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import { getContext } from "./api";

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
});
