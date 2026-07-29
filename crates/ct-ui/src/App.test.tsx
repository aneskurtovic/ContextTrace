import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import * as api from "./api";
import { demoContext, demoDetail, demoSessions } from "./demo";
import type {
  ContextDetail,
  SessionDetail,
  SessionPage,
  SessionSummary,
  StartupSummary,
} from "./types";

vi.mock("./api", () => ({
  getStartup: vi.fn(),
  listSessions: vi.fn(),
  searchSessions: vi.fn(),
  inspectSession: vi.fn(),
  getContext: vi.fn(),
}));

const startup: StartupSummary = {
  roots: [{ agent: "codex", paths: ["C:\\fixtures\\codex"] }],
  warnings: [],
};

const mockedApi = vi.mocked(api);

function sessionPage(
  sessions: SessionSummary[],
  total = sessions.length,
  offset = 0,
): SessionPage {
  return {
    sessions,
    total,
    offset,
    hasMore: offset + sessions.length < total,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((next) => {
    resolve = next;
  });
  return { promise, resolve };
}

beforeEach(() => {
  mockedApi.getStartup.mockResolvedValue(startup);
  mockedApi.searchSessions.mockResolvedValue(sessionPage([]));
  mockedApi.inspectSession.mockImplementation(async (id) => demoDetail(id));
  mockedApi.getContext.mockImplementation(async (_id, turn) => demoContext(turn));
});

afterEach(() => {
  cleanup();
  vi.resetAllMocks();
});

describe("desktop accessibility and state handling", () => {
  it("announces loading and renders a deterministic empty state", async () => {
    const sessions = deferred<SessionPage>();
    mockedApi.searchSessions.mockReturnValueOnce(sessions.promise);

    render(<App />);

    expect(
      screen.getByRole("navigation", { name: "Sessions" }).getAttribute("aria-busy"),
    ).toBe("true");
    expect(screen.getByText("Discovering local sessions…")).not.toBeNull();

    sessions.resolve(sessionPage([]));

    expect(await screen.findByText("No matching sessions")).not.toBeNull();
    expect(screen.getByRole("heading", { name: "Select a session" })).not.toBeNull();
    expect(
      screen.getByRole("navigation", { name: "Sessions" }).getAttribute("aria-busy"),
    ).toBe("false");
  });

  it("reports an API failure in an alert that can be dismissed", async () => {
    mockedApi.searchSessions.mockRejectedValueOnce(new Error("invalid response from session list"));

    render(<App />);

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("ContextTrace couldn’t complete that view.");
    expect(alert.textContent).toContain("invalid response from session list");

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
  });

  it("labels an empty but valid context response instead of leaving blank panels", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockResolvedValue({
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
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

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

  it("searches the complete backend catalog after the query debounce", async () => {
    mockedApi.searchSessions
      .mockResolvedValueOnce(sessionPage([demoSessions[0]]))
      .mockResolvedValueOnce(sessionPage([demoSessions[2]]));

    render(<App />);

    expect(
      await screen.findByRole("button", { name: /Codex session: ContextTrace/ }),
    ).not.toBeNull();
    fireEvent.change(screen.getByRole("searchbox", { name: "Search sessions" }), {
      target: { value: "semantic-search" },
    });

    await waitFor(
      () =>
        expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
          undefined,
          "semantic-search",
          0,
          200,
        ),
      { timeout: 1_000 },
    );
    expect(
      await screen.findByRole("button", { name: /Codex session: semantic-search/ }),
    ).not.toBeNull();
  });

  it("loads older sessions from an explicit next page", async () => {
    mockedApi.searchSessions
      .mockResolvedValueOnce(sessionPage([demoSessions[0]], 2))
      .mockResolvedValueOnce(sessionPage([demoSessions[1]], 2, 1));

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Load more (1)" }));

    await waitFor(() =>
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(undefined, "", 1, 200),
    );
    expect(
      await screen.findByRole("button", { name: /Claude Code session: atlas-dashboard/ }),
    ).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Load more/ })).toBeNull();
  });

  it("keeps the latest session visible when older detail and context requests finish later", async () => {
    const [first, latest] = demoSessions;
    const firstDetail = deferred<SessionDetail>();
    const latestDetail = deferred<SessionDetail>();
    const firstContext = deferred<ContextDetail>();
    const latestContext = deferred<ContextDetail>();
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([first, latest]));
    mockedApi.inspectSession.mockImplementation((id) =>
      id === first.id ? firstDetail.promise : latestDetail.promise,
    );
    mockedApi.getContext.mockImplementation((id) =>
      id === first.id ? firstContext.promise : latestContext.promise,
    );

    render(<App />);

    await waitFor(() => expect(mockedApi.inspectSession).toHaveBeenCalledWith(first.id));
    fireEvent.click(await screen.findByRole("button", { name: /Codex session: ContextTrace/ }));
    fireEvent.click(screen.getByRole("button", { name: /Claude Code session: atlas-dashboard/ }));
    await waitFor(() => expect(mockedApi.inspectSession).toHaveBeenCalledWith(latest.id));
    expect(await screen.findByText("Reading session…")).not.toBeNull();
    expect(screen.queryByRole("heading", { name: "ContextTrace" })).toBeNull();

    latestDetail.resolve(demoDetail(latest.id));
    latestContext.resolve(demoContext());
    expect(await screen.findByRole("heading", { name: "atlas-dashboard" })).not.toBeNull();

    firstDetail.resolve(demoDetail(first.id));
    firstContext.resolve(demoContext());
    await waitFor(() => expect(screen.getByRole("heading", { name: "atlas-dashboard" })).not.toBeNull());
    expect(screen.queryByRole("heading", { name: "ContextTrace" })).toBeNull();
  });

  it("keeps the latest turn context when responses arrive out of order", async () => {
    const firstTurn = deferred<ContextDetail>();
    const latestTurn = deferred<ContextDetail>();
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockImplementation((_id, turn) => {
      if (turn === 1) return firstTurn.promise;
      if (turn === 2) return latestTurn.promise;
      return Promise.resolve(demoContext(turn));
    });

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Inspect turn 1:/ }));
    fireEvent.click(screen.getByRole("button", { name: /Inspect turn 2:/ }));
    await waitFor(() => expect(mockedApi.getContext).toHaveBeenCalledWith(demoSessions[0].id, 2));

    latestTurn.resolve(demoContext(2));
    expect(await screen.findByLabelText("Inspect turn 2")).not.toBeNull();

    firstTurn.resolve(demoContext(1));
    await waitFor(() => expect(screen.getByLabelText("Inspect turn 2")).not.toBeNull());
  });

  it("identifies observed counts without claiming calibration", async () => {
    const observed = demoContext();
    observed.calibrationScale = null;
    observed.categories = observed.categories.map((category) => ({
      ...category,
      confidence: "observed",
    }));
    observed.contributors = observed.contributors.map((contributor) => ({
      ...contributor,
      confidence: "observed",
    }));
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockResolvedValueOnce(observed);

    render(<App />);

    expect(await screen.findByText(/Confidence labels: observed = logged/)).not.toBeNull();
    expect(screen.getAllByText("observed").length).toBeGreaterThan(0);
    expect(screen.queryByText(/Estimated items calibrated/)).toBeNull();
  });

  it("discloses calibrated estimated counts", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockResolvedValueOnce(demoContext());

    render(<App />);

    const measurement = await screen.findByText(/Confidence labels: observed = logged/);
    expect(measurement.textContent).toContain(
      "Estimated items calibrated 0.94× to the reported total.",
    );
    expect(measurement.textContent).toContain("estimated = modelled");
  });

  it("marks a non-meaningful residual as not measurable", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockResolvedValueOnce({
      ...demoContext(),
      calibrationScale: 0.8,
      residualTokens: 0,
      residualIsMeaningful: false,
    });

    render(<App />);

    expect(
      await screen.findByText(
        "Unattributed remainder: not measurable. Rows are proportions of the reported total, not a complete inventory.",
      ),
    ).not.toBeNull();
  });

  it("does not call a measurable zero residual unmeasurable", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockResolvedValueOnce({
      ...demoContext(),
      residualTokens: 0,
      residualIsMeaningful: true,
    });

    render(<App />);

    expect(
      await screen.findByText("Unattributed remainder: 0 tokens for this reconstruction."),
    ).not.toBeNull();
    expect(screen.queryByText(/Unattributed remainder: not measurable/)).toBeNull();
  });
});
