import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import App from "./App";
import * as api from "./api";
import {
  demoArchiveHolding,
  demoContext,
  demoDetail,
  demoDoctor,
  demoLifecycle,
  demoNotificationPage,
  demoNotificationSettings,
  demoNotificationStatus,
  demoResidual,
  demoSessions,
  demoTurnDiff,
} from "./demo";
import type {
  ContextDetail,
  SessionDetail,
  SessionPage,
  SessionSummary,
  StartupSummary,
  SessionUpdatedEvent,
} from "./types";

vi.mock("./api", () => ({
  isDemoData: vi.fn(),
  getStartup: vi.fn(),
  searchSessions: vi.fn(),
  inspectSession: vi.fn(),
  getContext: vi.fn(),
  runDoctor: vi.fn(),
  getLifecycle: vi.fn(),
  getResidual: vi.fn(),
  getTurnDiff: vi.fn(),
  getCompactionDiff: vi.fn(),
  archivedSessions: vi.fn(),
  archiveSession: vi.fn(),
  verifyArchived: vi.fn(),
  exportSession: vi.fn(),
  getNotificationSettings: vi.fn(),
  updateNotificationSettings: vi.fn(),
  getNotificationStatus: vi.fn(),
  listNotifications: vi.fn(),
  markNotificationsRead: vi.fn(),
  dismissNotification: vi.fn(),
  clearNotificationHistory: vi.fn(),
  listenForNotificationUpdates: vi.fn(),
  listenForSessionUpdates: vi.fn(),
}));

const startup: StartupSummary = {
  roots: [{ agent: "codex", paths: ["C:\\fixtures\\codex"] }],
  warnings: [],
  archiveRoot: "C:\\fixtures\\archive",
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

async function openView(name: "Overview" | "Turns" | "Diff" | "Evidence") {
  const tab = await screen.findByRole("tab", { name });
  await waitFor(() => expect(tab.hasAttribute("disabled")).toBe(false));
  fireEvent.click(tab);
  await waitFor(() => expect(tab.getAttribute("aria-selected")).toBe("true"));
  return tab;
}

beforeEach(() => {
  // These cases are about a real desktop read; the demonstration-data path has
  // its own file, which deliberately does not mock `./api`.
  mockedApi.isDemoData.mockReturnValue(false);
  mockedApi.getStartup.mockResolvedValue(startup);
  mockedApi.searchSessions.mockResolvedValue(sessionPage([]));
  mockedApi.inspectSession.mockImplementation(async (_agent, id) => demoDetail(id));
  mockedApi.getContext.mockImplementation(async (_agent, _id, turn) => demoContext(turn));
  mockedApi.runDoctor.mockImplementation(async (_agent, _id, turn) => demoDoctor(turn));
  mockedApi.getLifecycle.mockImplementation(async (_agent, _id, item) => demoLifecycle(item));
  mockedApi.getResidual.mockImplementation(async (agent, id) => demoResidual(agent, id));
  // Fetched unconditionally on mount, like `getStartup` -- every test needs a
  // resolved value here or the archive panel's load spins forever.
  mockedApi.archivedSessions.mockResolvedValue(demoArchiveHolding);
  mockedApi.getNotificationSettings.mockResolvedValue({ ...demoNotificationSettings, onboardingComplete: true });
  mockedApi.updateNotificationSettings.mockImplementation(async (settings) => settings);
  mockedApi.getNotificationStatus.mockResolvedValue(demoNotificationStatus);
  mockedApi.listNotifications.mockResolvedValue(demoNotificationPage);
  mockedApi.markNotificationsRead.mockResolvedValue(undefined);
  mockedApi.dismissNotification.mockResolvedValue(undefined);
  mockedApi.clearNotificationHistory.mockResolvedValue(undefined);
  mockedApi.listenForNotificationUpdates.mockResolvedValue(() => undefined);
  mockedApi.listenForSessionUpdates.mockResolvedValue(() => undefined);
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
    await waitFor(() =>
      expect(mockedApi.getContext).toHaveBeenCalledWith(demoSessions[0].agent, demoSessions[0].id, 1),
    );
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
          false,
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
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(undefined, "", 1, 200, false),
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
    mockedApi.inspectSession.mockImplementation((_agent, id) =>
      id === first.id ? firstDetail.promise : latestDetail.promise,
    );
    mockedApi.getContext.mockImplementation((_agent, id) =>
      id === first.id ? firstContext.promise : latestContext.promise,
    );

    render(<App />);

    await waitFor(() =>
      expect(mockedApi.inspectSession).toHaveBeenCalledWith(first.agent, first.id),
    );
    fireEvent.click(await screen.findByRole("button", { name: /Codex session: ContextTrace/ }));
    fireEvent.click(screen.getByRole("button", { name: /Claude Code session: atlas-dashboard/ }));
    await waitFor(() =>
      expect(mockedApi.inspectSession).toHaveBeenCalledWith(latest.agent, latest.id),
    );
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
    mockedApi.getContext.mockImplementation((_agent, _id, turn) => {
      if (turn === 1) return firstTurn.promise;
      if (turn === 2) return latestTurn.promise;
      return Promise.resolve(demoContext(turn));
    });

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /Inspect turn 1:/ }));
    fireEvent.click(screen.getByRole("button", { name: /Inspect turn 2:/ }));
    await waitFor(() =>
      expect(mockedApi.getContext).toHaveBeenCalledWith(demoSessions[0].agent, demoSessions[0].id, 2),
    );

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

  it("runs the deeper Context Doctor only after explicit consent", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const analyze = await screen.findByRole("button", { name: "Analyze turn 32" });
    expect(mockedApi.runDoctor).not.toHaveBeenCalled();
    fireEvent.click(analyze);

    await waitFor(() =>
      expect(mockedApi.runDoctor).toHaveBeenCalledWith(demoSessions[0].agent, demoSessions[0].id, 32),
    );
    expect((await screen.findAllByText("18.2k")).length).toBeGreaterThan(0);
    expect(screen.getByText("Repeated content")).not.toBeNull();
    expect(screen.getByText("Low-information blocks")).not.toBeNull();
    expect(screen.getByText("196 record(s) checked")).not.toBeNull();
  });

  it("opens and closes a contributor lifecycle without a terminal", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const contributor = await screen.findByRole("button", {
      name: /tool: shell_command → test output/,
    });
    expect(contributor.getAttribute("aria-expanded")).toBe("false");
    fireEvent.click(contributor);

    await waitFor(() =>
      expect(mockedApi.getLifecycle).toHaveBeenCalledWith(
        demoSessions[0].agent,
        demoSessions[0].id,
        "demo-0",
      ),
    );
    expect(contributor.getAttribute("aria-expanded")).toBe("true");
    expect(await screen.findByText("turns 9–32")).not.toBeNull();
    expect(screen.getByText("24 observed")).not.toBeNull();
    expect(screen.getByText(/Still present at the last scanned turn/)).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Close item lifecycle" }));
    expect(screen.queryByRole("heading", { name: "tool: shell_command → test output" })).toBeNull();
    expect(contributor.getAttribute("aria-expanded")).toBe("false");
  });

  it("only marks an explicit refresh click as cache-busting, not paging or query changes", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]], 2));

    render(<App />);

    // Initial catalog load must not ask the backend to treat its cache as stale.
    await waitFor(() =>
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(undefined, "", 0, 200, false),
    );

    fireEvent.click(screen.getByRole("button", { name: "Refresh sessions" }));
    await waitFor(() =>
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(undefined, "", 0, 200, true),
    );
  });

  it("keeps a same-id collision across agents from cross-selecting or cross-loading", async () => {
    const codexSession: SessionSummary = {
      ...demoSessions[0],
      id: "collision-id",
      agent: "codex",
      project: "collision-codex-project",
    };
    const claudeSession: SessionSummary = {
      ...demoSessions[1],
      id: "collision-id",
      agent: "claude-code",
      project: "collision-claude-project",
    };
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([codexSession, claudeSession]));

    render(<App />);

    const codexRow = await screen.findByRole("button", {
      name: /Codex session: collision-codex-project/,
    });
    const claudeRow = await screen.findByRole("button", {
      name: /Claude Code session: collision-claude-project/,
    });

    // The first session in the page is selected by default; only its row
    // highlights even though the other agent's session shares its id.
    await waitFor(() =>
      expect(mockedApi.inspectSession).toHaveBeenCalledWith("codex", "collision-id"),
    );
    expect(codexRow.getAttribute("aria-current")).toBe("true");
    expect(claudeRow.getAttribute("aria-current")).toBeNull();

    mockedApi.inspectSession.mockClear();
    fireEvent.click(claudeRow);

    await waitFor(() =>
      expect(mockedApi.inspectSession).toHaveBeenCalledWith("claude-code", "collision-id"),
    );
    expect(claudeRow.getAttribute("aria-current")).toBe("true");
    expect(codexRow.getAttribute("aria-current")).toBeNull();
  });

  it("measures unlogged context only when asked, and states the spread beside the ratio", async () => {
    const claudeSession = demoSessions.find((session) => session.agent === "claude-code")!;
    mockedApi.searchSessions.mockResolvedValue(sessionPage([claudeSession]));

    render(<App />);
    await openView("Turns");

    const run = await screen.findByRole("button", { name: "Measure this session" });
    // Selecting a session must not pay for a full-session reconstruction.
    expect(mockedApi.getResidual).not.toHaveBeenCalled();

    fireEvent.click(run);

    await waitFor(() =>
      expect(mockedApi.getResidual).toHaveBeenCalledWith(claudeSession.agent, claudeSession.id),
    );
    expect(await screen.findByText("3.42")).not.toBeNull();
    // The ratio never appears without the sample size and spread that qualify it.
    expect(screen.getByText("24 turn pairs · spread 1.19×")).not.toBeNull();
    expect(screen.getByRole("img", { name: "Unlogged context per turn" })).not.toBeNull();
    // An over-counted turn is stated as a count, not left as a silent gap.
    expect(screen.getByText("1 of 32")).not.toBeNull();
  });

  it("renders a refused fit as the answer instead of charting a fabricated line", async () => {
    const claudeSession = demoSessions.find((session) => session.agent === "claude-code")!;
    mockedApi.searchSessions.mockResolvedValue(sessionPage([claudeSession]));
    mockedApi.getResidual.mockResolvedValue({
      kind: "overCounted",
      charsPerToken: 2.84,
      pairsUsed: 19,
      dispersion: 1.62,
      turnsMeasured: 41,
    });

    render(<App />);
    await openView("Turns");

    fireEvent.click(await screen.findByRole("button", { name: "Measure this session" }));

    expect(
      await screen.findByText(/all 41 turns reconstruct to more content than their prompts held/),
    ).not.toBeNull();
    // No chart, and above all no zeroed remainder standing in for one.
    expect(screen.queryByRole("img", { name: "Unlogged context per turn" })).toBeNull();
    expect(screen.queryByText("Typical hidden constant")).toBeNull();
  });

  it("says a step a compaction explains is explained, and narrates only the rest", async () => {
    const claudeSession = demoSessions.find((session) => session.agent === "claude-code")!;
    mockedApi.searchSessions.mockResolvedValue(sessionPage([claudeSession]));
    const fitted = demoResidual("claude-code", claudeSession.id);
    if (fitted.kind !== "fitted") throw new Error("expected the fitted demo session");
    mockedApi.getResidual.mockResolvedValue({
      ...fitted,
      steps: fitted.steps.filter((step) => step.nearCompaction),
    });

    render(<App />);
    await openView("Turns");

    fireEvent.click(await screen.findByRole("button", { name: "Measure this session" }));

    expect(
      await screen.findByText("A compaction occurred here, which explains it."),
    ).not.toBeNull();
    // With every step already accounted for by the log, attributing one to an
    // unrecorded harness change would invent a second cause for one event.
    expect(screen.queryByText(/a tool registered, an MCP server connected/)).toBeNull();
  });
  it("exposes the redesigned views as an accessible tab set", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);

    const overview = await screen.findByRole("tab", { name: "Overview" });
    await waitFor(() => expect(overview.hasAttribute("disabled")).toBe(false));
    expect(overview.getAttribute("aria-selected")).toBe("true");

    const turns = await openView("Turns");
    expect(turns.getAttribute("aria-selected")).toBe("true");
    expect(overview.getAttribute("aria-selected")).toBe("false");
    expect(await screen.findByRole("button", { name: "Analyze turn 32" })).not.toBeNull();
  });

  it("opens the command palette from the keyboard and toggles the theme", async () => {
    render(<App />);

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    expect(palette).not.toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Switch to light theme" }));
    expect(document.querySelector(".app-shell")?.getAttribute("data-theme")).toBe("light");
    expect(screen.getByRole("button", { name: "Switch to dark theme" })).not.toBeNull();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    expect(await screen.findByRole("dialog", { name: "Command palette" })).not.toBeNull();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull();
  });

  it("traps focus, restores it on close, and navigates actions with arrow keys", async () => {
    render(<App />);

    const trigger = screen.getByRole("button", { name: /Search sessions, turns, actions/ });
    trigger.focus();
    fireEvent.click(trigger);

    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    const search = screen.getByRole("textbox", { name: "Command palette search" });
    await waitFor(() => expect(document.activeElement).toBe(search));

    const actions = Array.from(
      palette.querySelectorAll<HTMLButtonElement>(".command-results button"),
    );
    expect(actions.length).toBeGreaterThan(2);

    fireEvent.keyDown(search, { key: "ArrowDown" });
    expect(document.activeElement).toBe(actions[0]);
    expect(actions[0].classList.contains("active")).toBe(true);

    fireEvent.keyDown(actions[0], { key: "ArrowDown" });
    expect(document.activeElement).toBe(actions[1]);

    fireEvent.keyDown(actions[1], { key: "ArrowUp" });
    expect(document.activeElement).toBe(actions[0]);

    fireEvent.keyDown(actions[0], { key: "ArrowUp" });
    expect(document.activeElement).toBe(actions.at(-1));

    fireEvent.keyDown(actions.at(-1)!, { key: "Tab" });
    expect(document.activeElement).toBe(search);

    fireEvent.keyDown(search, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(actions.at(-1));

    fireEvent.keyDown(actions.at(-1)!, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Command palette" })).toBeNull());
    expect(document.activeElement).toBe(trigger);
  });
});

describe('notifications', () => {
  it('opens the feed, marks a finding read, and navigates to its session turn', async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);

    const bell = await screen.findByRole('button', { name: 'Notifications, 2 unread' });
    fireEvent.click(bell);
    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });
    expect(drawer.textContent).toContain('2 unread');

    fireEvent.click(drawer.querySelectorAll('.notification-card')[1]);
    await waitFor(() => expect(mockedApi.markNotificationsRead).toHaveBeenCalledWith(['demo-notification-compaction']));
    await waitFor(() => expect(mockedApi.getContext).toHaveBeenCalledWith('codex', demoSessions[0].id, 18));
    expect(screen.queryByRole('dialog', { name: 'Notifications' })).toBeNull();
  });

  it('requires an explicit local-monitoring onboarding decision', async () => {
    mockedApi.getNotificationSettings.mockResolvedValue({
      ...demoNotificationSettings,
      onboardingComplete: false,
      enabled: false,
    });
    render(<App />);

    expect(await screen.findByRole('heading', { name: 'Know when a session needs attention' })).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Enable monitoring' }));
    await waitFor(() => expect(mockedApi.updateNotificationSettings).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: true, onboardingComplete: true }),
    ));
  });

  it('refreshes a followed session only after its matching backend event', async () => {
    let update: ((event: SessionUpdatedEvent) => void) | undefined;
    mockedApi.listenForSessionUpdates.mockImplementation(async (callback) => {
      update = callback;
      return () => undefined;
    });
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);

    fireEvent.click(await screen.findByRole('button', { name: 'Follow live' }));
    await waitFor(() => expect(update).toBeDefined());
    const before = mockedApi.inspectSession.mock.calls.length;
    update!({ agent: 'codex', sessionId: demoSessions[0].id });
    await waitFor(() => expect(mockedApi.inspectSession.mock.calls.length).toBe(before + 1));
    expect(mockedApi.searchSessions.mock.calls.some((call) => call[4] === true)).toBe(false);
  });
});

describe("the two panels that write", () => {
  /** The first session auto-selects, so waiting for the archive panel's own
   *  control is enough to know the workspace has rendered. */
  async function openFirstSession() {
    const session = demoSessions[0];
    mockedApi.searchSessions.mockResolvedValue(sessionPage([session]));
    render(<App />);
    await openView("Evidence");
    await screen.findByRole("button", { name: /to the archive$/ });
    return session;
  }

  it("archives redacted unless the opt-out is ticked, and never the other way round", async () => {
    const session = await openFirstSession();
    mockedApi.archiveSession.mockResolvedValue(demoArchiveHolding.entries[0]);

    fireEvent.click(await screen.findByRole("button", { name: /Add .* to the archive/ }));
    await waitFor(() => expect(mockedApi.archiveSession).toHaveBeenCalled());
    // The third argument is `raw`. Defaulting it the other way would make
    // the safe choice the one a user has to find.
    expect(mockedApi.archiveSession).toHaveBeenLastCalledWith(session.agent, session.id, false);

    fireEvent.click(
      screen.getByRole("checkbox", { name: /Keep credentials instead of redacting them/ }),
    );
    fireEvent.click(screen.getByRole("button", { name: /Add .* to the archive/ }));
    await waitFor(() => expect(mockedApi.archiveSession).toHaveBeenCalledTimes(2));
    expect(mockedApi.archiveSession).toHaveBeenLastCalledWith(session.agent, session.id, true);
  });

  it("exports redacted unless the opt-out is ticked, unlike ct export's own default", async () => {
    const session = await openFirstSession();
    mockedApi.exportSession.mockResolvedValue({
      path: "C:\\fixtures\\archive\\exports\\codex\\session.ndjson",
      bytes: 4096,
      records: 128,
      redaction: "secrets",
      redactions: 0,
    });

    fireEvent.click(await screen.findByRole("button", { name: "Export to NDJSON" }));
    await waitFor(() => expect(mockedApi.exportSession).toHaveBeenCalled());
    // `redactSecrets` is the third argument, and the desktop writes into a
    // directory it chose rather than to a pipe the user chose.
    expect(mockedApi.exportSession).toHaveBeenLastCalledWith(session.agent, session.id, true);

    fireEvent.click(screen.getByRole("checkbox", { name: /Keep recognised credentials/ }));
    fireEvent.click(screen.getByRole("button", { name: "Export to NDJSON" }));
    await waitFor(() => expect(mockedApi.exportSession).toHaveBeenCalledTimes(2));
    expect(mockedApi.exportSession).toHaveBeenLastCalledWith(session.agent, session.id, false);
  });

  it("reports where an export landed and what it did about credentials", async () => {
    await openFirstSession();
    mockedApi.exportSession.mockResolvedValue({
      path: "C:\\fixtures\\archive\\exports\\codex\\session.ndjson",
      bytes: 4096,
      records: 128,
      redaction: "secrets",
      redactions: 3,
    });

    fireEvent.click(await screen.findByRole("button", { name: "Export to NDJSON" }));

    expect(
      await screen.findByText("C:\\fixtures\\archive\\exports\\codex\\session.ndjson"),
    ).not.toBeNull();
    expect(screen.getByText("scanned — 3 value(s) replaced")).not.toBeNull();
  });

  it("says a kept-credentials export holds whatever the log held", async () => {
    await openFirstSession();
    mockedApi.exportSession.mockResolvedValue({
      path: "C:\\fixtures\\archive\\exports\\codex\\session.ndjson",
      bytes: 4096,
      records: 128,
      redaction: "none",
      redactions: 0,
    });

    fireEvent.click(screen.getByRole("checkbox", { name: /Keep recognised credentials/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Export to NDJSON" }));

    // A zero replacement count must never be what tells the user this file is
    // clean: nothing was replaced because nothing was looked for.
    expect(
      await screen.findByText("kept — this file holds whatever the log held"),
    ).not.toBeNull();
  });

  it("offers no way to open an archived session, and says so rather than staying silent", async () => {
    await openFirstSession();

    const rows = await screen.findAllByRole("listitem");
    const archived = rows.filter((row) => row.className.includes("archive-row"));
    expect(archived.length).toBe(demoArchiveHolding.entries.length);
    // Exactly one control per row, and it verifies rather than opens. A row
    // that reads as clickable and does nothing is worse than the CLI here.
    for (const row of archived) {
      const buttons = row.querySelectorAll("button");
      expect(buttons.length).toBe(1);
      expect(buttons[0].textContent).toBe("Verify");
    }
    expect(screen.getByText(/Nothing reads a copy back yet/)).not.toBeNull();
  });

  it("names the directory it writes to in both panels that write to it", async () => {
    await openFirstSession();

    expect(await screen.findByText(demoArchiveHolding.root)).not.toBeNull();
    expect(screen.getByText(`${demoArchiveHolding.root}\\exports`)).not.toBeNull();
  });
});

describe("comparing turns across two sessions", () => {
  it("drops the pinned baseline when a different session is opened", async () => {
    const [codex, claude] = [demoSessions[0], demoSessions.find((s) => s.agent === "claude-code")!];
    mockedApi.searchSessions.mockResolvedValue(sessionPage([codex, claude]));

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /^Pin this turn/ }));
    expect(await screen.findByText(/^Baseline pinned at/)).not.toBeNull();

    fireEvent.click(
      screen.getByRole("button", { name: new RegExp(`session: .*${claude.id.slice(0, 8)}`) }),
    );

    // A pin is a bare turn number. Carried across, it would rebase onto the
    // new session at a turn chosen for a different one -- which this session
    // may not even have.
    await waitFor(() => expect(screen.queryByText(/^Baseline pinned at/)).toBeNull());
    expect(await screen.findByRole("button", { name: /^Pin this turn/ })).not.toBeNull();
  });

  it("withholds token deltas when the two sides were sized by different instruments", async () => {
    const [codex, claude] = [demoSessions[0], demoSessions.find((s) => s.agent === "claude-code")!];
    mockedApi.searchSessions.mockResolvedValue(sessionPage([codex, claude]));
    mockedApi.getTurnDiff.mockImplementation(async (left, right) => demoTurnDiff(left, right));

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: /^Pin this turn/ }));
    await openView("Diff");
    fireEvent.change(await screen.findByRole("combobox"), {
      target: { value: JSON.stringify({ agent: claude.agent, id: claude.id }) },
    });
    fireEvent.change(screen.getByRole("spinbutton"), { target: { value: "12" } });

    await waitFor(() => expect(mockedApi.getTurnDiff).toHaveBeenCalled());
    const [left, right] = mockedApi.getTurnDiff.mock.calls.at(-1)!;
    expect(left.agent).toBe(codex.agent);
    expect(right.agent).toBe(claude.agent);
    expect(right.turn).toBe(12);

    // `incomparable` is not "skew zero". Across two agents no factor relates
    // the scales, so the deltas are withheld rather than widened -- and this
    // arm was unreachable from the desktop until the diff could name two
    // sessions at all.
    expect(await screen.findByText(/do not log the same things/)).not.toBeNull();
  });
});
