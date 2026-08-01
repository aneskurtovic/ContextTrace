import { afterEach, describe, expect, it, vi } from "vitest";
import { cleanup, render, screen } from "@testing-library/react";
import App from "./App";

// `./api` is deliberately *not* mocked here. This file exercises the branch the
// rest of the suite mocks away: with no Tauri bridge on `window`, every api
// function answers from the fabricated fixtures in `demo.ts`, and the interface
// has to say so rather than present invented sessions as a local read.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(() => {
    throw new Error("the desktop bridge is not available");
  }),
}));

afterEach(() => {
  cleanup();
});

describe("desktop without the Tauri bridge", () => {
  it("labels fabricated data wherever it is rendered", async () => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

    render(<App />);

    expect(
      await screen.findByText(
        /every session, token count and finding on this screen is fabricated/,
      ),
    ).not.toBeNull();
    expect(screen.getByText("Demonstration sessions")).not.toBeNull();

    // The footer must not claim a local read while showing invented numbers.
    expect(screen.queryByText("Reads local logs. No network.")).toBeNull();
    expect(screen.getByText("Demonstration data")).not.toBeNull();

    // The workspace fills with demo figures once the first session resolves.
    expect(await screen.findByText("Demo data")).not.toBeNull();
    expect(screen.queryByText("Local only")).toBeNull();
  });
});
