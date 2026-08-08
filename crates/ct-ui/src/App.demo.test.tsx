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

  it("refuses to write rather than pretending it wrote something", async () => {
    delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

    render(<App />);

    // Archiving and exporting are this app's only writes. With no bridge there
    // is no session to copy and no directory to copy it into, so the controls
    // are disabled and say why — a fabricated success here would be the one
    // demo claim a user could act on and be wrong about.
    const archive = await screen.findByRole("button", { name: /to the archive$/ });
    expect(archive.hasAttribute("disabled")).toBe(true);
    const write = screen.getByRole("button", { name: "Export to NDJSON" });
    expect(write.hasAttribute("disabled")).toBe(true);
    expect(screen.getAllByText(/Demo mode has nothing to write/).length).toBe(2);
  });
});
