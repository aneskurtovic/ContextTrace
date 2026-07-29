import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import * as api from "./api";
import { demoContext, demoDetail, demoSessions } from "./demo";
import type { StartupSummary } from "./types";

vi.mock("./api", () => ({
  getStartup: vi.fn(),
  listSessions: vi.fn(),
  inspectSession: vi.fn(),
  getContext: vi.fn(),
}));

const startup: StartupSummary = {
  roots: [{ agent: "codex", paths: ["C:\\fixtures\\codex"] }],
  warnings: [],
};

const mockedApi = vi.mocked(api);

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

beforeEach(() => {
  mockedApi.getStartup.mockResolvedValue(startup);
  mockedApi.listSessions.mockResolvedValue([]);
  mockedApi.inspectSession.mockImplementation(async (id) => demoDetail(id));
  mockedApi.getContext.mockImplementation(async (_id, turn) => demoContext(turn));
});

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("desktop accessibility and state handling", () => {
  it("announces loading and renders a deterministic empty state", async () => {
    const sessions = deferred<typeof demoSessions>();
    mockedApi.listSessions.mockReturnValueOnce(sessions.promise);

    render(<App />);

    expect(
      screen.getByRole("navigation", { name: "Sessions" }).getAttribute("aria-busy"),
    ).toBe("true");
    expect(screen.getByText("Discovering local sessions…")).not.toBeNull();

    sessions.resolve([]);

    expect(await screen.findByText("No matching sessions")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Select a session" })).not.toBeNull();
    expect(
      screen.getByRole("navigation", { name: "Sessions" }).getAttribute("aria-busy"),
    ).toBe("false");
  });

  it("reports an API failure in an alert that can be dismissed", async () => {
    mockedApi.listSessions.mockRejectedValueOnce(new Error("invalid response from session list"));

    render(<App />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("ContextTrace couldn’t complete that view.");
    expect(alert.textContent).toContain("invalid response from session list");

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("labels an empty but valid context response instead of leaving blank panels", async () => {
    mockedApi.listSessions.mockResolvedValueOnce([demoSessions[0]]);
    mockedApi.getContext.mockResolvedValueOnce({
      ...demoContext(),
      categories: [],
      contributors: [],
    });

    render(<App />);

    expect(
      await screen.findByText("No context categories were reported for this turn."),
    ).not.toBeNull();
    expect(
      screen.getByText("No individual contributors were reported for this turn."),
    ).not.toBeNull();
  });

  it("exposes filters, selected sessions, roots, and chart turns to keyboard users", async () => {
    mockedApi.listSessions.mockResolvedValueOnce([demoSessions[0]]);

    render(<App />);

    const filters = screen.getByRole("group", { name: "Filter sessions by agent" });
    expect(filters).not.toBeNull();
    expect(screen.getByRole("button", { name: "All" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    expect(screen.getByRole("button", { name: "Codex" }).getAttribute("aria-pressed")).toBe(
      "false",
    );

    const session = await screen.findByRole("button", { name: /Codex session: ContextTrace/ });
    expect(session.getAttribute("aria-current")).toBe("true");

    const roots = screen.getByRole("button", { name: /Private by design/ });
    expect(roots.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(roots);
    expect(roots.getAttribute("aria-expanded")).toBe("true");
    expect(screen.getByText("C:\\fixtures\\codex")).not.toBeNull();

    const chartTurn = await screen.findByRole("button", { name: /Inspect turn 1:/ });
    fireEvent.keyDown(chartTurn, { key: "Enter" });
    await waitFor(() => expect(mockedApi.getContext).toHaveBeenCalledWith(demoSessions[0].id, 1));
  });
});
