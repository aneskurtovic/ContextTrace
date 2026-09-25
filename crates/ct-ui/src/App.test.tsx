import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import * as api from "./api";
import {
  demoCost,
  demoCompactionDiff,
  demoArchiveHolding,
  demoContext,
  demoCorpus,
  demoDetail,
  demoDoctor,
  demoInstructionFiles,
  demoLifecycle,
  demoNotificationPage,
  demoNotificationSettings,
  demoNotificationStatus,
  demoResidual,
  demoSessions,
  demoTemporalGhost,
  demoTranscript,
  demoTranscriptEntry,
  demoTurnDiff,
} from "./demo";
import type {
  ContextDetail,
  CorpusReport,
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
  listProjects: vi.fn(),
  inspectSession: vi.fn(),
  getContext: vi.fn(),
  getCorpus: vi.fn(),
  getCorpusCached: vi.fn(),
  listenForCorpusProgress: vi.fn(),
  getTranscript: vi.fn(),
  getTranscriptEntry: vi.fn(),
  runDoctor: vi.fn(),
  getLifecycle: vi.fn(),
  getResidual: vi.fn(),
  getTurnDiff: vi.fn(),
  getTemporalGhost: vi.fn(),
  getCompactionDiff: vi.fn(),
  getCost: vi.fn(),
  getInstructionFiles: vi.fn(),
  archivedSessions: vi.fn(),
  archiveSession: vi.fn(),
  verifyArchived: vi.fn(),
  exportSession: vi.fn(),
  getNotificationSettings: vi.fn(),
  updateNotificationSettings: vi.fn(),
  getNotificationStatus: vi.fn(),
  sendTestNotification: vi.fn(),
  listNotifications: vi.fn(),
  markNotificationsRead: vi.fn(),
  dismissNotification: vi.fn(),
  clearNotificationHistory: vi.fn(),
  listenForNotificationUpdates: vi.fn(),
  listenForSessionUpdates: vi.fn(),
  searchMemory: vi.fn(),
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

/** Leave the corpus view, which is what the app now opens on. */
function leaveCorpus() {
  const corpus = screen.queryByRole("button", { name: "All sessions" });
  if (corpus?.getAttribute("aria-pressed") === "true") fireEvent.click(corpus);
}

async function openView(name: "Overview" | "Turns" | "Chat" | "Diff" | "Evidence") {
  // The session tab strip does not exist while the corpus is showing, so
  // leaving it is a precondition of every view assertion rather than
  // something each test has to remember.
  leaveCorpus();
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
  mockedApi.listProjects.mockResolvedValue([]);
  mockedApi.inspectSession.mockImplementation(async (_agent, id) => demoDetail(id));
  mockedApi.getContext.mockImplementation(async (_agent, _id, turn) => demoContext(turn));
  mockedApi.getCorpus.mockResolvedValue(demoCorpus);
  // No remembered sweep by default: tests that care about the cached-first
  // paint say so, and the rest should exercise the real sweep path.
  mockedApi.getCorpusCached.mockResolvedValue(null);
  mockedApi.listenForCorpusProgress.mockResolvedValue(() => undefined);
  mockedApi.getTranscript.mockImplementation(async (_agent, _id, offset = 0, limit = 40) =>
    demoTranscript(offset, limit),
  );
  mockedApi.getTranscriptEntry.mockImplementation(async (_agent, _id, index) =>
    demoTranscriptEntry(index),
  );
  mockedApi.runDoctor.mockImplementation(async (_agent, _id, turn) => demoDoctor(turn));
  mockedApi.getLifecycle.mockImplementation(async (_agent, _id, item) => demoLifecycle(item));
  mockedApi.getResidual.mockImplementation(async (agent, id) => demoResidual(agent, id));
  mockedApi.getTurnDiff.mockImplementation(async (left, right) => demoTurnDiff(left, right));
  mockedApi.getTemporalGhost.mockImplementation(async (_agent, _id, leftTurn, rightTurn) =>
    demoTemporalGhost(leftTurn, rightTurn),
  );
  mockedApi.getCompactionDiff.mockImplementation(async (agent, _id, lineNo) =>
    demoCompactionDiff(agent, lineNo),
  );
  mockedApi.getInstructionFiles.mockImplementation(async (_agent, id) => demoInstructionFiles(id));
  mockedApi.getCost.mockImplementation(async (_agent, id, _pricing, forecastTurns) =>
    demoCost(id, forecastTurns ?? 0),
  );
  // Fetched unconditionally on mount, like `getStartup` -- every test needs a
  // resolved value here or the archive panel's load spins forever.
  mockedApi.archivedSessions.mockResolvedValue(demoArchiveHolding);
  mockedApi.getNotificationSettings.mockResolvedValue({ ...demoNotificationSettings, onboardingComplete: true });
  mockedApi.updateNotificationSettings.mockImplementation(async (settings) => settings);
  mockedApi.getNotificationStatus.mockResolvedValue(demoNotificationStatus);
  mockedApi.sendTestNotification.mockResolvedValue({
    delivered: true,
    reason: null,
    deliverability: { state: 'ready', appId: 'dev.contexttrace.desktop' },
  });
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

afterEach(() => window.localStorage.clear());

describe("desktop accessibility and state handling", () => {
  it("shows the models used in both Overview and Chat", async () => {
    const detail = demoDetail(demoSessions[0].id);
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.inspectSession.mockResolvedValueOnce({
      ...detail,
      modelUsage: [
        { model: "gpt-5.4", turns: 3 },
        { model: "gpt-5.3", turns: 1 },
      ],
      unattributedModelTurns: 1,
    });

    render(<App />);
    await openView("Overview");

    const overview = document.getElementById("overview-panel");
    expect(overview).not.toBeNull();
    expect(within(overview!).getByRole("heading", { name: "Models used" })).not.toBeNull();
    expect(within(overview!).getByText("gpt-5.4")).not.toBeNull();
    expect(within(overview!).getByText("gpt-5.3")).not.toBeNull();
    expect(within(overview!).getByText("1 turn did not record a model.")).not.toBeNull();

    await openView("Chat");
    const chat = document.getElementById("chat-panel");
    expect(chat).not.toBeNull();
    expect(within(chat!).getByLabelText("Models used")).not.toBeNull();
    expect(within(chat!).getByText("gpt-5.4")).not.toBeNull();
    expect(within(chat!).getByText("gpt-5.3")).not.toBeNull();
  });

  it("announces loading and renders a deterministic empty state", async () => {
    const sessions = deferred<SessionPage>();
    mockedApi.searchSessions.mockReturnValueOnce(sessions.promise);

    render(<App />);
    leaveCorpus();

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
    leaveCorpus();

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
    leaveCorpus();

    const filters = screen.getByRole("group", { name: "Filter sessions by agent" });
    expect(filters).not.toBeNull();
    expect(screen.getByRole("button", { name: "All" }).getAttribute("aria-pressed")).toBe(
      "true",
    );
    expect(screen.getByRole("button", { name: "Codex" }).getAttribute("aria-pressed")).toBe(
      "false",
    );

    const session = await screen.findByRole("button", { name: /Codex session: .*ContextTrace/ });
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
      await screen.findByRole("button", { name: /Codex session: .*ContextTrace/ }),
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
          { kind: "any" },
          false,
        ),
      { timeout: 1_000 },
    );
    expect(
      await screen.findByRole("button", { name: /Codex session: .*semantic-search/ }),
    ).not.toBeNull();
  });

  it("loads older sessions from an explicit next page", async () => {
    mockedApi.searchSessions
      .mockResolvedValueOnce(sessionPage([demoSessions[0]], 2))
      .mockResolvedValueOnce(sessionPage([demoSessions[1]], 2, 1));

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Load more (1)" }));

    await waitFor(() =>
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
        undefined, "", 1, 200, false, { kind: "any" }, false,
      ),
    );
    expect(
      await screen.findByRole("button", { name: /Claude Code session: .*atlas-dashboard/ }),
    ).not.toBeNull();
    expect(screen.queryByRole("button", { name: /Load more/ })).toBeNull();
  });

  it("narrows the session list by project and by thread role", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage(demoSessions));
    mockedApi.listProjects.mockResolvedValue([
      { path: "C:\\work\\api", label: "api", count: 3 },
      { path: null, label: "No recorded folder", count: 1 },
    ]);
    render(<App />);

    // demoSessions carries several Codex rows, so wait for the list rather
    // than a single accessible name that would be ambiguous once loaded.
    await screen.findAllByRole("button", { name: /Codex session/ });

    fireEvent.change(screen.getByLabelText("Filter sessions by project"), {
      target: { value: "C:\\work\\api" },
    });
    await waitFor(() => expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
      undefined, "", 0, 200, false, { kind: "path", path: "C:\\work\\api" }, false,
    ));

    fireEvent.click(screen.getByLabelText("Show subagent sessions"));
    await waitFor(() => expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
      undefined, "", 0, 200, false, { kind: "path", path: "C:\\work\\api" }, true,
    ));
  });

  it("threads the search box query into the project dropdown's counts", async () => {
    // Regression: the dropdown used to call listProjects with no query at
    // all, so it kept reading e.g. "contexttrace · 41" after the search box
    // had already narrowed the visible session list to a handful of rows --
    // a number that described a set the search box had excluded.
    mockedApi.searchSessions.mockResolvedValue(sessionPage(demoSessions));
    mockedApi.listProjects.mockResolvedValue([]);
    render(<App />);

    await screen.findAllByRole("button", { name: /Codex session/ });
    mockedApi.listProjects.mockClear();

    fireEvent.change(screen.getByLabelText("Search sessions"), {
      target: { value: "atlas" },
    });

    await waitFor(
      () => expect(mockedApi.listProjects).toHaveBeenLastCalledWith(undefined, "atlas", false),
      { timeout: 1_000 },
    );
  });

  it("resets an orphaned project filter when the agent filter changes", async () => {
    // Regression: a project chosen under one agent can be meaningless under
    // another. Left in place, the backend returned zero sessions and the
    // <select> rendered blank -- its value named an option absent from its
    // own list -- with nothing on screen explaining the empty result.
    mockedApi.searchSessions.mockResolvedValue(sessionPage(demoSessions));
    mockedApi.listProjects.mockResolvedValue([
      { path: "C:\\work\\api", label: "api", count: 3 },
    ]);
    render(<App />);

    await screen.findAllByRole("button", { name: /Codex session/ });
    const select = screen.getByLabelText("Filter sessions by project") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "C:\\work\\api" } });
    await waitFor(() => expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
      undefined, "", 0, 200, false, { kind: "path", path: "C:\\work\\api" }, false,
    ));
    expect(select.value).toBe("C:\\work\\api");

    fireEvent.click(screen.getByRole("button", { name: "Codex" }));

    await waitFor(() => expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
      "codex", "", 0, 200, false, { kind: "any" }, false,
    ));
    expect(select.value).toBe("any");
  });

  it("surfaces a project list failure in the alert instead of emptying the dropdown", async () => {
    // Regression: `.catch(() => setProjectOptions([]))` made a backend
    // failure indistinguishable from "you have exactly one project" (the
    // ever-present "All projects" choice would be all that remained). The
    // fix reports the failure and keeps whatever options were already shown.
    mockedApi.searchSessions.mockResolvedValue(sessionPage(demoSessions));
    mockedApi.listProjects.mockResolvedValueOnce([
      { path: "C:\\work\\api", label: "api", count: 3 },
    ]);
    render(<App />);

    await screen.findAllByRole("button", { name: /Codex session/ });
    const select = screen.getByLabelText("Filter sessions by project") as HTMLSelectElement;
    expect(select.options.length).toBe(2); // "All projects" plus "api"

    mockedApi.listProjects.mockRejectedValueOnce(new Error("project list unavailable"));
    fireEvent.click(screen.getByRole("button", { name: "Codex" }));

    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("project list unavailable");
    expect(select.options.length).toBe(2);
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
    fireEvent.click(await screen.findByRole("button", { name: /Codex session: .*ContextTrace/ }));
    fireEvent.click(screen.getByRole("button", { name: /Claude Code session: .*atlas-dashboard/ }));
    await waitFor(() =>
      expect(mockedApi.inspectSession).toHaveBeenCalledWith(latest.agent, latest.id),
    );
    // The workspace heading is the session's own name, so these assertions
    // name the two sessions rather than the two projects they ran in -- which
    // is the point of the change: two sessions in one repository used to give
    // this heading the same text twice.
    const firstName = first.title!.text;
    const latestName = latest.title!.text;
    expect(await screen.findByText("Reading session…")).not.toBeNull();
    expect(screen.queryByRole("heading", { name: firstName })).toBeNull();

    latestDetail.resolve(demoDetail(latest.id));
    latestContext.resolve(demoContext());
    expect(await screen.findByRole("heading", { name: latestName })).not.toBeNull();

    firstDetail.resolve(demoDetail(first.id));
    firstContext.resolve(demoContext());
    await waitFor(() => expect(screen.getByRole("heading", { name: latestName })).not.toBeNull());
    expect(screen.queryByRole("heading", { name: firstName })).toBeNull();
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
    leaveCorpus();

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
    leaveCorpus();

    expect(await screen.findByText(/Confidence labels: observed = logged/)).not.toBeNull();
    expect(screen.getAllByText("observed").length).toBeGreaterThan(0);
    expect(screen.queryByText(/Estimated items calibrated/)).toBeNull();
  });

  it("discloses calibrated estimated counts", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getContext.mockResolvedValueOnce(demoContext());

    render(<App />);
    leaveCorpus();

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
    leaveCorpus();

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
    leaveCorpus();

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
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
        undefined, "", 0, 200, false, { kind: "any" }, false,
      ),
    );

    fireEvent.click(screen.getByRole("button", { name: "Refresh sessions" }));
    await waitFor(() =>
      expect(mockedApi.searchSessions).toHaveBeenLastCalledWith(
        undefined, "", 0, 200, true, { kind: "any" }, false,
      ),
    );
  });

  it("tells two sessions in one repository apart by name, branch and title provenance", async () => {
    // The complaint this answers: every row in a repository read
    // `contexttrace · a1b2c3d4`, so the catalog could not be scanned. Both
    // sessions below share a project on purpose.
    const [titled, untitled] = [
      { ...demoSessions[0], id: "same-repo-one" },
      {
        ...demoSessions[0],
        id: "same-repo-two",
        title: { text: "Ship the notification delivery fix", source: "firstPrompt" as const },
        gitBranch: "fix/toasts",
      },
    ];
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([titled, untitled]));

    render(<App />);

    const rows = await screen.findAllByRole("button", { name: /Codex session:/ });
    expect(rows[0].textContent).toContain("Trace the 38k-token tool result in the planner");
    expect(rows[1].textContent).toContain("Ship the notification delivery fix");
    expect(rows[1].textContent).toContain("fix/toasts");
    // Both name a session; only one is the agent's own summary of it, and the
    // mark is what stops the weaker claim from reading as the stronger.
    expect(rows[0].querySelector(".title-source")).toBeNull();
    expect(rows[1].querySelector(".title-source")).not.toBeNull();
  });

  it("falls back to the project when a session has no name of its own", async () => {
    const nameless: SessionSummary = { ...demoSessions[0], title: null, gitBranch: null };
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([nameless]));

    render(<App />);

    const row = await screen.findByRole("button", { name: /Codex session:/ });
    expect(row.querySelector(".session-title")!.textContent).toBe("ContextTrace");
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
      name: /Codex session: .*collision-codex-project/,
    });
    const claudeRow = await screen.findByRole("button", {
      name: /Claude Code session: .*collision-claude-project/,
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

  it("measures unlogged context automatically when Turns opens, and states the spread beside the ratio", async () => {
    const claudeSession = demoSessions.find((session) => session.agent === "claude-code")!;
    mockedApi.searchSessions.mockResolvedValue(sessionPage([claudeSession]));

    render(<App />);
    await openView("Turns");

    // Opening the tab is itself the question; no click is required to ask it.
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

    expect(
      await screen.findByText("A compaction occurred here, which explains it."),
    ).not.toBeNull();
    // With every step already accounted for by the log, attributing one to an
    // unrecorded harness change would invent a second cause for one event.
    expect(screen.queryByText(/a tool registered, an MCP server connected/)).toBeNull();
  });

  it("measures the session when Turns opens, once per session", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage(demoSessions.slice(0, 2)));
    render(<App />);

    await openView("Turns");
    await waitFor(() => expect(mockedApi.getResidual).toHaveBeenCalledTimes(1));

    // Leaving and returning is not a new question about the same session.
    await openView("Overview");
    await openView("Turns");
    await waitFor(() => expect(mockedApi.getResidual).toHaveBeenCalledTimes(1));
  });

  it("measures a newly selected session even though the previous one was already measured", async () => {
    const [first, second] = demoSessions;
    mockedApi.searchSessions.mockResolvedValue(sessionPage([first, second]));
    render(<App />);

    await openView("Turns");
    await waitFor(() =>
      expect(mockedApi.getResidual).toHaveBeenCalledWith(first.agent, first.id),
    );

    // A different session is a new question, even though Turns is already open.
    const secondRow = await screen.findByRole("button", { name: new RegExp(second.title!.text) });
    fireEvent.click(secondRow);
    await waitFor(() =>
      expect(mockedApi.getResidual).toHaveBeenCalledWith(second.agent, second.id),
    );
    expect(mockedApi.getResidual).toHaveBeenCalledTimes(2);
  });

  it("exposes the redesigned views as an accessible tab set", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    leaveCorpus();

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

    const trigger = screen.getByRole("button", { name: /Commands and navigation/ });
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

describe("corpus overview", () => {
  it("paints the remembered sweep first, then replaces it with the real one", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));

    // A fingerprint is all-or-nothing, so one session that grew by a line
    // sends the dashboard back to a full sweep. The view the app opens on
    // cannot begin with seconds of spinner, so last time's numbers go up
    // first -- labelled as last time's.
    mockedApi.getCorpusCached.mockResolvedValue({
      ...demoCorpus,
      sessions: 391,
      cached: true,
    });
    const sweep = deferred<CorpusReport>();
    mockedApi.getCorpus.mockReturnValueOnce(sweep.promise);

    render(<App />);

    // Remembered numbers, on screen while the sweep is still running.
    expect(await screen.findByRole("heading", { name: "All sessions" })).not.toBeNull();
    expect(screen.getByText("391", { selector: "strong" })).not.toBeNull();
    expect(screen.getByText(/from the last sweep/)).not.toBeNull();
    expect(screen.getByText(/re-reading now/)).not.toBeNull();

    sweep.resolve({ ...demoCorpus, sessions: 392, cached: false });

    // ...and replaced once it lands, with the caveat gone.
    expect(await screen.findByRole("heading", { name: "All sessions" })).not.toBeNull();
    expect(screen.getByText("392", { selector: "strong" })).not.toBeNull();
    await waitFor(() => expect(screen.queryByText(/from the last sweep/)).toBeNull());
  });
  it("summarises every session and states what it could not measure", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);


    expect(await screen.findByRole("heading", { name: "All sessions" })).not.toBeNull();
    expect(screen.getByText("134", { selector: "strong" })).not.toBeNull();
    // A cost total without its unpriced count reads as complete when it is a
    // floor, and a corpus with no measured compaction is not a corpus where
    // compaction freed nothing. Both caveats have to be on screen.
    expect(screen.getByText(/2,046 turns had no local rate/)).not.toBeNull();
    expect(screen.getByText(/no before\/after sizes recorded/i)).not.toBeNull();
    expect(
      screen.getByText(/53 of 134 sessions never recorded both a prompt size/),
    ).not.toBeNull();

    // The session-scoped tab strip is meaningless here and is gone, not
    // disabled.
    expect(screen.queryByRole("tab", { name: "Overview" })).toBeNull();
  });

  it("reuses the last sweep until asked to re-run it", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);
    await screen.findByRole("heading", { name: "All sessions" });

    expect(mockedApi.getCorpus).toHaveBeenCalledWith(false);

    fireEvent.click(screen.getByRole("button", { name: "Re-sweep" }));

    await waitFor(() => expect(mockedApi.getCorpus).toHaveBeenCalledWith(true));
  });
});

describe("conversation", () => {
  it("reads a session back with tool results collapsed to their size", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);
    await openView("Chat");

    const entries = await screen.findAllByRole("listitem");
    const conversation = entries.filter((entry) => entry.className.includes("transcript-entry"));
    expect(conversation.length).toBeGreaterThan(3);

    // The point of the view: a 152,480-character tool result sits between an
    // ordinary question and an ordinary answer, and is collapsed to its size
    // rather than pasted.
    const result = conversation.find((entry) => entry.className.includes("toolResult"))!;
    expect(result.textContent).toContain("152,480 chars");
    expect(result.querySelector(".transcript-text")).toBeNull();
    expect(result.querySelector(".transcript-collapsed")).not.toBeNull();

    // A message is not machinery and arrives open.
    const message = conversation.find((entry) => entry.className.includes("user"))!;
    expect(message.querySelector(".transcript-text")!.textContent).toContain(
      "losing track of the schema",
    );
  });

  it("fetches the rest of a truncated entry only when it is expanded", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);
    await openView("Chat");

    const result = (await screen.findAllByRole("listitem")).find((entry) =>
      entry.className.includes("toolResult"),
    )!;
    expect(mockedApi.getTranscriptEntry).not.toHaveBeenCalled();

    fireEvent.click(result.querySelector<HTMLButtonElement>(".transcript-toggle")!);

    await waitFor(() =>
      expect(mockedApi.getTranscriptEntry).toHaveBeenCalledWith("codex", demoSessions[0].id, 4),
    );
    await waitFor(() =>
      expect(result.querySelector(".transcript-text")!.textContent!.length).toBeGreaterThan(2_000),
    );
  });

  it("moves from a message to the measurements of the turn it belongs to", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);
    await openView("Chat");

    // The link back is what makes this a transcript rather than a chat log:
    // the reader who spots the oversized result goes straight to the turn's
    // composition.
    fireEvent.click((await screen.findAllByRole("button", { name: "turn 1" }))[0]);

    await waitFor(() => expect(mockedApi.getContext).toHaveBeenCalledWith("codex", demoSessions[0].id, 1));
    expect(screen.getByRole("tab", { name: "Turns" }).getAttribute("aria-selected")).toBe("true");
    expect(await screen.findByRole("heading", { name: "What filled the context window" })).not.toBeNull();
  });

  it("stops a failed transcript expansion and offers an explicit retry", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    mockedApi.getTranscriptEntry
      .mockRejectedValueOnce(new Error("temporary disk read failure"))
      .mockImplementationOnce(async (_agent, _id, index) => demoTranscriptEntry(index));
    render(<App />);
    await openView("Chat");
    const result = (await screen.findAllByRole("listitem")).find((entry) => entry.className.includes("toolResult"))!;

    fireEvent.click(result.querySelector<HTMLButtonElement>(".transcript-toggle")!);
    expect((await within(result).findByRole("alert")).textContent).toContain("temporary disk read failure");
    expect(mockedApi.getTranscriptEntry).toHaveBeenCalledTimes(1);

    fireEvent.click(within(result).getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(mockedApi.getTranscriptEntry).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(result.querySelector(".transcript-text")!.textContent!.length).toBeGreaterThan(2_000));
  });

  it("ignores stale analysis results after switching sessions and clears loading", async () => {
    const oldAnalysis = deferred<Awaited<ReturnType<typeof demoInstructionFiles>>>();
    const currentAnalysis = deferred<Awaited<ReturnType<typeof demoInstructionFiles>>>();
    const first = demoSessions[0];
    const second = demoSessions[1];
    mockedApi.searchSessions.mockResolvedValue(sessionPage([first, second]));
    mockedApi.getInstructionFiles.mockReturnValueOnce(oldAnalysis.promise).mockReturnValueOnce(currentAnalysis.promise);
    render(<App />);
    await openView("Turns");
    fireEvent.click(screen.getByRole("button", { name: "Check instruction files" }));
    fireEvent.click(screen.getByRole("button", { name: new RegExp(`session: .*${second.id.slice(0, 8)}`) }));

    const checkButton = await screen.findByRole("button", { name: "Check instruction files" });
    expect((checkButton as HTMLButtonElement).disabled).toBe(false);
    fireEvent.click(checkButton);
    currentAnalysis.resolve({ ...demoInstructionFiles(second.id), comparisons: [{
      ...demoInstructionFiles(second.id).comparisons[0], detail: "CURRENT SESSION RESULT",
    }] });
    expect(await screen.findByText("CURRENT SESSION RESULT")).not.toBeNull();
    oldAnalysis.resolve({
      ...demoInstructionFiles(first.id),
      comparisons: [{ ...demoInstructionFiles(first.id).comparisons[0], detail: "STALE RESULT SHOULD NOT APPEAR" }],
    });
    await waitFor(() => expect(screen.queryByText("STALE RESULT SHOULD NOT APPEAR")).toBeNull());
    expect(screen.getByText("CURRENT SESSION RESULT")).not.toBeNull();
    expect((checkButton as HTMLButtonElement).disabled).toBe(false);
  });

  it("routes a compaction marker to the visible diff autopsy", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);
    await openView("Overview");

    fireEvent.click(screen.getByRole("button", { name: /Inspect compaction at turn 17/ }));

    expect(screen.getByRole("tab", { name: "Diff" }).getAttribute("aria-selected")).toBe("true");
    const heading = await screen.findByRole("heading", { name: /What turn 17's compaction replaced/ });
    await waitFor(() => expect(document.activeElement).toBe(heading));
  });

  it("deep-links content search hits to the highlighted transcript record", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    mockedApi.searchMemory.mockResolvedValue([{
      sessionId: demoSessions[0].id,
      agent: "codex",
      project: demoSessions[0].project,
      line: 10,
      turn: 1,
      preview: "needle in a tool result",
    }]);
    render(<App />);

    fireEvent.change(screen.getByRole("searchbox", { name: "Search sessions" }), { target: { value: "needle" } });
    const hit = await screen.findByRole("button", { name: /needle in a tool result/ });
    fireEvent.click(hit);

    expect(screen.getByRole("tab", { name: "Chat" }).getAttribute("aria-selected")).toBe("true");
    await waitFor(() => expect(document.querySelector(".transcript-entry.highlighted")).not.toBeNull());
  });

  it("labels the 50-result search cap and exposes all fetched hits", async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    mockedApi.searchMemory.mockResolvedValue(Array.from({ length: 50 }, (_, index) => ({
      sessionId: demoSessions[0].id, agent: "codex" as const, project: demoSessions[0].project,
      line: index + 1, turn: 1, preview: `search hit ${index + 1}`,
    })));
    render(<App />);
    fireEvent.change(screen.getByRole("searchbox", { name: "Search sessions" }), { target: { value: "needle" } });
    expect(await screen.findByText("50 (cap)")).not.toBeNull();
    expect(screen.getByText(/capped count, not the total/)).not.toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Show remaining 44 fetched hits" }));
    expect(screen.getByRole("button", { name: /search hit 50/ })).not.toBeNull();
  });
});

describe('notifications', () => {
  it('contains keyboard focus in the drawer and restores it to the opener', async () => {
    render(<App />);
    const bell = await screen.findByRole('button', { name: /^Notifications/ });
    fireEvent.click(bell);
    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });
    expect(document.querySelector('.app-content')?.hasAttribute('inert')).toBe(true);
    const settings = within(drawer).getByRole('button', { name: 'Settings' });
    await waitFor(() => expect(document.activeElement).toBe(settings));

    fireEvent.keyDown(settings, { key: 'Tab', shiftKey: true });
    const tabbable = Array.from(drawer.querySelectorAll<HTMLElement>(
      'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])',
    ));
    expect(document.activeElement).toBe(tabbable.at(-1));
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' });
    await waitFor(() => expect(screen.queryByRole('dialog', { name: 'Notifications' })).toBeNull());
    expect(document.querySelector('.app-content')?.hasAttribute('inert')).toBe(false);
    expect(document.activeElement).toBe(bell);
  });

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

  it('follows a secret finding to the record it names, across a page boundary', async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));

    // `scrollIntoView` scrolls every scrollable ancestor, and the app shell is
    // one of them -- calling it slid the top bar, search and notification bell
    // off the top of the window. The row has to move its own pane instead, so
    // reaching for this API at all is the regression.
    const previousScrollIntoView = Element.prototype.scrollIntoView;
    const scrolledEveryAncestor = vi.fn();
    Element.prototype.scrollIntoView = scrolledEveryAncestor;
    try {

    const entry = (index: number, line: number, text: string) => ({
      index, line, text,
      kind: 'toolResult' as const,
      turn: 25,
      label: null,
      truncated: false,
      chars: text.length,
      sidechain: false,
      error: false,
      collapsed: true,
    });

    // The named record sits on the second page. A highlight that only lands
    // when the entry happens to be in the first fetch is not a highlight.
    mockedApi.getTranscript.mockImplementation(async (_agent, _id, offset = 0) =>
      offset === 0
        ? { entries: [entry(0, 12, 'an earlier record')], total: 2, offset: 0, hasMore: true }
        : { entries: [entry(1, 191, 'assignment to UserSecretEncrypted')], total: 2, offset: 1, hasMore: false });

    mockedApi.listNotifications.mockResolvedValue({
      ...demoNotificationPage,
      unreadCount: 1,
      notifications: [{
        ...demoNotificationPage.notifications[1],
        id: 'test-notification-secret',
        ruleId: 'secretExposure' as const,
        severity: 'critical' as const,
        title: 'Secret entered model context',
        description: 'secret-like assignment detected at line 191.',
        readAt: null,
        location: {
          agent: demoSessions[0].agent,
          sessionId: demoSessions[0].id,
          project: demoSessions[0].project,
          turn: 25,
          sourceLine: 191,
        },
      }],
    });

    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: /^Notifications/ }));
    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });
    fireEvent.click(drawer.querySelectorAll('.notification-card')[0]);

    // Reaching the record at all requires the second page to be pulled in.
    const text = await screen.findByText(/UserSecretEncrypted/);
    await waitFor(() =>
      expect(text.closest('.transcript-entry')!.className).toContain('highlighted'));
    expect(scrolledEveryAncestor).not.toHaveBeenCalled();
    } finally {
      Element.prototype.scrollIntoView = previousScrollIntoView;
    }
  });
  it('drops a record highlight when the reader moves to another session', async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0], demoSessions[1]]));

    const entry = (index: number, line: number, text: string) => ({
      index, line, text,
      kind: 'toolResult' as const,
      turn: 25,
      label: null,
      truncated: false,
      chars: text.length,
      sidechain: false,
      error: false,
      collapsed: true,
    });

    // The session the reader moves to always claims another page. A highlight
    // that is not tied to the session it came from would chase line 191
    // through every page of a conversation the alert was never about.
    const calls: string[] = [];
    mockedApi.getTranscript.mockImplementation(async (_agent, id, offset = 0) => {
      calls.push(id + ':' + offset);
      if (id === demoSessions[0].id) {
        return offset === 0
          ? { entries: [entry(0, 12, 'an earlier record')], total: 2, offset: 0, hasMore: true }
          : { entries: [entry(1, 191, 'assignment to UserSecretEncrypted')], total: 2, offset: 1, hasMore: false };
      }
      return { entries: [entry(0, 5, 'an unrelated conversation')], total: 500, offset, hasMore: true };
    });

    mockedApi.listNotifications.mockResolvedValue({
      ...demoNotificationPage,
      unreadCount: 1,
      notifications: [{
        ...demoNotificationPage.notifications[1],
        id: 'test-notification-secret-switch',
        ruleId: 'secretExposure' as const,
        severity: 'critical' as const,
        readAt: null,
        location: {
          agent: demoSessions[0].agent,
          sessionId: demoSessions[0].id,
          project: demoSessions[0].project,
          turn: 25,
          sourceLine: 191,
        },
      }],
    });

    render(<App />);
    fireEvent.click(await screen.findByRole('button', { name: /^Notifications/ }));
    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });
    fireEvent.click(drawer.querySelectorAll('.notification-card')[0]);
    await screen.findByText(/UserSecretEncrypted/);

    fireEvent.click(document.querySelectorAll('.session-row')[1]);
    await screen.findByText(/an unrelated conversation/);

    const others = calls.filter((call) => call.startsWith(demoSessions[1].id));
    expect(others).toEqual([demoSessions[1].id + ':0']);
    expect(document.querySelectorAll('.transcript-entry.highlighted')).toHaveLength(0);
  });
  it('says which findings reached the OS and why the others did not', async () => {
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);

    fireEvent.click(await screen.findByRole('button', { name: 'Notifications, 2 unread' }));
    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });

    // The demo feed carries one failed OS delivery and one that was never
    // requested. A row that says nothing at all is what let 38 undelivered
    // toasts read as delivered, so the failure has to be on the row.
    expect(drawer.textContent).toContain('OS failed');
    expect(drawer.textContent).toContain('no OS notification was sent in demo mode');
    expect(drawer.querySelectorAll('.os-delivered')).toHaveLength(0);
  });

  it('reports a real outcome for a test notification instead of assuming one', async () => {
    render(<App />);

    fireEvent.click(await screen.findByRole('button', { name: /^Notifications/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Settings' }));
    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });

    // Deliverability, not the plugin's permission answer: on Windows that is
    // `granted` whatever the truth is, so the obstacle is the honest half.
    expect(drawer.querySelector('.notification-health')!.textContent).toContain('unsupported');
    expect(screen.getByText(/OS notifications cannot be delivered from this build/)).not.toBeNull();

    mockedApi.sendTestNotification.mockResolvedValue({
      delivered: false,
      reason: 'no installed shortcut carries the app id dev.contexttrace.desktop',
      deliverability: { state: 'unregistered', appId: 'dev.contexttrace.desktop', exeDir: null },
    });
    fireEvent.click(screen.getByRole('button', { name: 'Send test notification' }));

    expect(await screen.findByText(/no installed shortcut carries the app id/)).not.toBeNull();
  });

  it('explains an empty settings panel instead of redrawing the feed', async () => {
    mockedApi.getNotificationSettings.mockRejectedValue(new Error('store unreadable'));
    render(<App />);

    fireEvent.click(await screen.findByRole('button', { name: /^Notifications/ }));
    fireEvent.click(screen.getByRole('button', { name: /Settings/ }));

    // A control that changes nothing when pressed is worse than one that says why.
    expect(await screen.findByText(/store unreadable/)).not.toBeNull();
    expect(screen.getByRole('button', { name: 'Retry loading settings' })).not.toBeNull();
    const drawer = screen.getByRole('dialog', { name: 'Notifications' });
    expect(drawer.querySelector('.notification-feed')).toBeNull();
  });

  it('requires an explicit local-monitoring onboarding decision', async () => {
    mockedApi.getNotificationSettings.mockResolvedValue({
      ...demoNotificationSettings,
      onboardingComplete: false,
      enabled: false,
    });
    render(<App />);

    expect(await screen.findByRole('heading', { name: 'Know when a session needs attention' })).not.toBeNull();
    const onboarding = screen.getByRole('dialog', { name: 'Know when a session needs attention' });
    await waitFor(() => expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Enable monitoring' })));
    expect(document.querySelector('.app-content')?.hasAttribute('inert')).toBe(true);
    fireEvent.keyDown(window, { key: 'k', ctrlKey: true });
    expect(screen.queryByRole('dialog', { name: 'Command palette' })).toBeNull();
    expect(onboarding).not.toBeNull();
    fireEvent.click(screen.getByRole('button', { name: 'Enable monitoring' }));
    await waitFor(() => expect(mockedApi.updateNotificationSettings).toHaveBeenCalledWith(
      expect.objectContaining({ enabled: true, onboardingComplete: true }),
    ));
  });

  it('still loads the settings when the status payload is rejected', async () => {
    // One bad payload used to reject the whole Promise.all, which left settings
    // null -- and the panel is gated on settings, so pressing Settings redrew
    // the feed and looked like a dead button.
    mockedApi.getNotificationStatus.mockRejectedValue(
      new Error('ContextTrace received an invalid response from notification status.'),
    );
    render(<App />);

    fireEvent.click(await screen.findByRole('button', { name: /^Notifications/ }));
    fireEvent.click(screen.getByRole('button', { name: /Settings/ }));

    const drawer = await screen.findByRole('dialog', { name: 'Notifications' });
    expect(drawer.querySelector('.notification-settings')).not.toBeNull();
    expect(drawer.querySelectorAll('.notification-rule')).toHaveLength(11);
  });

  it('refreshes a followed session only after its matching backend event', async () => {
    let update: ((event: SessionUpdatedEvent) => void) | undefined;
    mockedApi.listenForSessionUpdates.mockImplementation(async (callback) => {
      update = callback;
      return () => undefined;
    });
    mockedApi.searchSessions.mockResolvedValue(sessionPage([demoSessions[0]]));
    render(<App />);
    leaveCorpus();

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
    leaveCorpus();

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
    leaveCorpus();

    fireEvent.click(await screen.findByRole("button", { name: /^Pin this turn/ }));
    await openView("Diff");
    // The sidebar's own project filter is a second combobox on screen once
    // this view is open, so the cross-session picker needs its label to
    // disambiguate which one the change targets.
    fireEvent.change(
      await screen.findByRole("combobox", { name: "Compare this turn with" }),
      { target: { value: JSON.stringify({ agent: claude.agent, id: claude.id }) } },
    );
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

describe("composition items and drill-down", () => {
  it("expands and collapses category rows, showing items on expand and hiding on collapse", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    // Find all category buttons and locate tool-outputs by looking for one with the right item count
    const buttons = await screen.findAllByRole("button");
    const toolOutputsButton = buttons.find((btn) => {
      const text = btn.textContent;
      return text && text.includes("Tool outputs") && text.includes("18");
    });
    expect(toolOutputsButton).not.toBeUndefined();

    const categoryButton = toolOutputsButton!;
    expect(categoryButton.getAttribute("aria-expanded")).toBe("false");

    // Expand the row
    fireEvent.click(categoryButton);
    expect(categoryButton.getAttribute("aria-expanded")).toBe("true");

    // After expanding, check that composition items are now visible
    // The text might appear in multiple places (item list and contributor list),
    // so check that at least one appears in a composition-items container
    const allInstances = await screen.findAllByText(/tool: shell_command → test output/);
    const inCompositionItems = allInstances.some((el) => el.closest(".composition-items"));
    expect(inCompositionItems).toBe(true);

    // Collapse it
    fireEvent.click(categoryButton);
    expect(categoryButton.getAttribute("aria-expanded")).toBe("false");
    await waitFor(() => {
      const items = screen.queryAllByText(/tool: shell_command → test output/);
      // Should only be in the contributors list now, not in composition items
      const compositionItemsStillVisible = items.some((el) =>
        el.closest(".composition-items")
      );
      expect(compositionItemsStillVisible).toBe(false);
    });
  });

  it("opens only one category at a time, closing the previous when opening a new one", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const buttons = await screen.findAllByRole("button");
    const toolButton = buttons.find((btn) => {
      const text = btn.textContent;
      return text && text.includes("Tool outputs") && text.includes("18");
    });
    const assistantButton = buttons.find((btn) => {
      const text = btn.textContent;
      return text && text.includes("Assistant messages") && text.includes("21");
    });

    expect(toolButton).not.toBeUndefined();
    expect(assistantButton).not.toBeUndefined();

    fireEvent.click(toolButton!);
    expect(toolButton!.getAttribute("aria-expanded")).toBe("true");
    expect(assistantButton!.getAttribute("aria-expanded")).toBe("false");

    fireEvent.click(assistantButton!);
    expect(assistantButton!.getAttribute("aria-expanded")).toBe("true");
    expect(toolButton!.getAttribute("aria-expanded")).toBe("false");
  });

  it("shows up to 8 items and displays a truncation message when itemCount exceeds the preview limit", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const context = demoContext();
    const toolOutputsItems = context.items.filter((i) => i.category === "tool-outputs");

    expect(toolOutputsItems.length).toBeGreaterThan(8);

    // Find and click the tool-outputs category button
    const categoryButton = screen.getByRole("button", {
      name: (accessibleName) => accessibleName.includes("18") && accessibleName.includes("items"),
    });
    fireEvent.click(categoryButton);

    // Check that the truncation message appears with the correct count
    const remaining = toolOutputsItems.length - 8;
    expect(
      await screen.findByText(new RegExp(`… ${remaining} smaller items`)),
    ).not.toBeNull();
  });

  it("shows the unattributed explanatory sentence instead of an empty list", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const context = demoContext();
    const unattributedCategory = context.categories.find(
      (c) => c.category === "unattributed",
    );
    expect(unattributedCategory).not.toBeNull();

    // Find the unattributed button by looking for the "Unattributed" text
    const categoryButton = screen.getByRole("button", {
      name: (accessibleName) => accessibleName.includes("Unattributed"),
    });
    fireEvent.click(categoryButton);

    // The unattributed row has no items behind it, so it should show the explanatory sentence
    expect(
      await screen.findByText(
        /This row is the remainder of the reported total, not a logged item, so there is nothing to list/,
      ),
    ).not.toBeNull();
  });
});

describe("contributor detail panel", () => {
  it("renders contributor detail panel and calls getLifecycle when selected", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const context = demoContext();
    const firstContributor = context.contributors[0];

    // Find and click the first contributor button by partial label match
    const buttons = await screen.findAllByRole("button");
    const contributorButton = buttons.find((btn) => {
      const text = btn.textContent;
      return text && text.includes(firstContributor.label.substring(0, 15));
    });
    expect(contributorButton).not.toBeUndefined();

    fireEvent.click(contributorButton!);

    // Verify getLifecycle was called with the correct contributor
    await waitFor(() =>
      expect(mockedApi.getLifecycle).toHaveBeenCalledWith(
        demoSessions[0].agent,
        demoSessions[0].id,
        firstContributor.id,
      ),
    );
  });
});

describe("Find hidden changes gating and baseline controls", () => {
  // The reconstruction needs two turns and two *different* ones. The button
  // used to be disabled on the first condition alone, with nothing on screen
  // saying so and the pin that satisfies it living in another panel; a turn
  // pinned to the turn already on screen then left the button enabled and the
  // click silently doing nothing. Each state is asserted through the button's
  // own `disabled` and `title` rather than by locating text, because every tab
  // panel stays in the DOM behind `hidden` and a loose text query passes from
  // whichever view happens to hold the string.
  const ghostButton = async () =>
    await screen.findByRole("button", { name: "Find hidden changes" });

  it("refuses to compare, and says why, until a baseline turn is pinned", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    const button = await ghostButton();
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(true));
    expect(button.getAttribute("title")).toBe(
      "Pin a baseline turn, then step to another turn to compare it against.",
    );
    expect(
      await screen.findByRole("button", { name: /^Pin turn \d+ as baseline$/ }),
    ).not.toBeNull();
  });

  it("stays refused when the pinned baseline is the turn already on screen", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    fireEvent.click(await screen.findByRole("button", { name: /^Pin turn \d+ as baseline$/ }));

    const button = await ghostButton();
    await waitFor(() =>
      expect(button.getAttribute("title")).toMatch(
        /^Turn \d+ is both the baseline and the turn on screen\./,
      ),
    );
    expect(button.hasAttribute("disabled")).toBe(true);
    expect(mockedApi.getTemporalGhost).not.toHaveBeenCalled();
  });

  it("compares the pinned baseline with the turn on screen once the two differ", async () => {
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));

    render(<App />);
    await openView("Turns");

    fireEvent.click(await screen.findByRole("button", { name: /^Pin turn \d+ as baseline$/ }));
    const pinned = Number(
      /Unpin turn (\d+)/.exec(
        (await screen.findByRole("button", { name: /^Unpin turn \d+$/ })).textContent ?? "",
      )![1],
    );

    fireEvent.click(screen.getByRole("button", { name: "Previous measured turn" }));

    const button = await ghostButton();
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
    fireEvent.click(button);

    await waitFor(() => expect(mockedApi.getTemporalGhost).toHaveBeenCalled());
    const [, , left, right] = mockedApi.getTemporalGhost.mock.calls.at(-1)!;
    expect(left).toBe(pinned);
    expect(right).not.toBe(pinned);
  });

  it("clears a pending reconstruction when the selected turn changes", async () => {
    const pending = deferred<Awaited<ReturnType<typeof demoTemporalGhost>>>();
    mockedApi.searchSessions.mockResolvedValueOnce(sessionPage([demoSessions[0]]));
    mockedApi.getTemporalGhost.mockReturnValueOnce(pending.promise);
    render(<App />);
    await openView("Turns");
    fireEvent.click(await screen.findByRole("button", { name: /^Pin turn \d+ as baseline$/ }));
    fireEvent.click(screen.getByRole("button", { name: "Previous measured turn" }));
    const button = await ghostButton();
    await waitFor(() => expect(button.hasAttribute("disabled")).toBe(false));
    fireEvent.click(button);
    await waitFor(() => expect(button.textContent).toContain("Reconstructing"));
    fireEvent.click(screen.getByRole("button", { name: "Next measured turn" }));
    await waitFor(() => expect(button.textContent).toBe("Find hidden changes"));
    expect(button.hasAttribute("disabled")).toBe(true);
    pending.resolve(demoTemporalGhost(18, 17));
    await waitFor(() => expect(screen.queryByText("Temporal ghost")).toBeNull());
  });
});

describe("resizing the sessions panel", () => {
  it('remembers a sidebar width and refuses a stored one it could not show', async () => {
    window.localStorage.setItem('ct.sidebarWidth', '9000');
    render(<App />);

    // An out-of-range stored value must not restore a panel the user cannot see.
    const shell = document.querySelector('.app-shell') as HTMLElement;
    await waitFor(() => expect(shell.style.getPropertyValue('--sidebar-width')).toBe('250px'));

    const separator = screen.getByRole('separator', { name: 'Resize the sessions panel' });
    fireEvent.keyDown(separator, { key: 'ArrowRight' });
    expect(shell.style.getPropertyValue('--sidebar-width')).toBe('266px');
    expect(window.localStorage.getItem('ct.sidebarWidth')).toBe('266');
  });
});
