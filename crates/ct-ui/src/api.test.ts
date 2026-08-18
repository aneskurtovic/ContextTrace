import { afterEach, describe, expect, it, vi } from "vitest";

const { invoke, listen } = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock('@tauri-apps/api/event', () => ({ listen }));

import {
  archivedSessions,
  archiveSession,
  exportSession,
  getCost,
  getCompactionDiff,
  getContext,
  getInstructionFiles,
  getLifecycle,
  getNotificationSettings,
  getNotificationStatus,
  getResidual,
  getStartup,
  getTemporalGhost,
  getTurnDiff,
  inspectSession,
  listNotifications,
  listenForNotificationUpdates,
  listenForSessionUpdates,
  listProjects,
  markNotificationsRead,
  runDoctor,
  searchSessions,
  sendTestNotification,
  verifyArchived,
} from "./api";
import {
  demoCost,
  demoArchiveHolding,
  demoArchiveVerification,
  demoCompactionDiff,
  demoContext,
  demoDetail,
  demoDoctor,
  demoInstructionFiles,
  demoNotificationPage,
  demoNotificationSettings,
  demoNotificationStatus,
  demoResidual,
  demoSessions,
  demoTemporalGhost,
  demoTurnDiff,
} from "./demo";

afterEach(() => {
  delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  invoke.mockReset();
  listen.mockReset();
});

describe('notification IPC contracts', () => {
  it('validates complete settings, status, and history payloads', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValueOnce(demoNotificationSettings);
    await expect(getNotificationSettings()).resolves.toEqual(demoNotificationSettings);
    invoke.mockResolvedValueOnce(demoNotificationStatus);
    await expect(getNotificationStatus()).resolves.toEqual(demoNotificationStatus);
    invoke.mockResolvedValueOnce(demoNotificationPage);
    await expect(listNotifications(null, 30, false)).resolves.toEqual(demoNotificationPage);
    expect(invoke).toHaveBeenLastCalledWith('list_notifications', {
      beforeId: null,
      limit: 30,
      unreadOnly: false,
    });
  });

  it('rejects a settings payload that omits any stable rule', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const { contextPressure: _missing, ...rules } = demoNotificationSettings.rules;
    invoke.mockResolvedValue({ ...demoNotificationSettings, rules });
    await expect(getNotificationSettings()).rejects.toThrow('invalid response from notification settings');
  });

  it('refuses a status or record that leaves OS delivery unstated', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const { deliverability: _absent, ...status } = demoNotificationStatus;
    invoke.mockResolvedValueOnce(status);
    await expect(getNotificationStatus()).rejects.toThrow('invalid response from notification status');

    // An `unregistered` state that omits `exeDir`, and a `failed` delivery
    // without a reason, are both the shape this pass exists to prevent: a
    // verdict with nothing behind it. `null` is a stated absence and allowed;
    // a missing field is not.
    invoke.mockResolvedValueOnce({
      ...demoNotificationStatus,
      deliverability: { state: 'unregistered', appId: 'dev.contexttrace.desktop' },
    });
    await expect(getNotificationStatus()).rejects.toThrow('invalid response from notification status');

    const [record] = demoNotificationPage.notifications;
    invoke.mockResolvedValueOnce({
      ...demoNotificationPage,
      notifications: [{ ...record, osDelivery: { status: 'failed' } }],
    });
    await expect(listNotifications()).rejects.toThrow('invalid response from notification history');
  });

  it('reports a test notification outcome, and never fabricates one without the bridge', async () => {
    const undelivered = await sendTestNotification();
    expect(undelivered.delivered).toBe(false);
    expect(invoke).not.toHaveBeenCalled();

    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const outcome = {
      delivered: false,
      reason: 'no installed shortcut carries the app id',
      deliverability: { state: 'unregistered', appId: 'dev.contexttrace.desktop', exeDir: null },
    };
    invoke.mockResolvedValueOnce(outcome);
    await expect(sendTestNotification()).resolves.toEqual(outcome);
    expect(invoke).toHaveBeenCalledWith('send_test_notification');

    invoke.mockResolvedValueOnce({ delivered: true, reason: null });
    await expect(sendTestNotification()).rejects.toThrow('invalid response from a test notification result');
  });

  it('uses null to mark every notification read', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue(undefined);
    await markNotificationsRead();
    expect(invoke).toHaveBeenCalledWith('mark_notifications_read', { ids: null });
  });

  it('validates notification and session event payloads before dispatch', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const handlers = new Map<string, (event: { payload: unknown }) => void>();
    listen.mockImplementation(async (name, callback) => {
      handlers.set(name, callback);
      return () => undefined;
    });
    const notificationCallback = vi.fn();
    const sessionCallback = vi.fn();
    await listenForNotificationUpdates(notificationCallback);
    await listenForSessionUpdates(sessionCallback);

    handlers.get('contexttrace://notification-created')!({ payload: demoNotificationPage.notifications[0] });
    expect(notificationCallback).toHaveBeenCalledOnce();
    handlers.get('contexttrace://session-updated')!({ payload: { agent: 'codex', sessionId: 'abc' } });
    expect(sessionCallback).toHaveBeenCalledWith({ agent: 'codex', sessionId: 'abc' });
    expect(() => handlers.get('contexttrace://notification-updated')!({ payload: { id: 'unsafe-partial' } }))
      .toThrow('invalid response from a notification');
  });
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
      project: null,
      includeSubagents: false,
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

  it('sends the project filter in the shape the backend deserialises', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({ sessions: [], total: 0, offset: 0, hasMore: false });

    await searchSessions(undefined, undefined, 0, 200, false, { kind: 'path', path: 'C:\\work\\api' }, true);
    expect(invoke).toHaveBeenLastCalledWith('search_sessions', expect.objectContaining({
      project: { path: 'C:\\work\\api' },
      includeSubagents: true,
    }));

    // "Every project" and "the ones with no recorded folder" are different
    // requests, and null cannot mean both.
    await searchSessions(undefined, undefined, 0, 200, false, { kind: 'unrecorded' }, false);
    expect(invoke).toHaveBeenLastCalledWith('search_sessions', expect.objectContaining({
      project: 'unrecorded',
    }));

    await searchSessions(undefined, undefined, 0, 200, false, { kind: 'any' }, false);
    expect(invoke).toHaveBeenLastCalledWith('search_sessions', expect.objectContaining({
      project: null,
    }));
  });

  it('rejects a project list that is missing its counts', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue([{ path: 'C:\\work\\api', label: 'api' }]);
    await expect(listProjects()).rejects.toThrow('invalid response from the project list');
  });

  it('forwards listProjects arguments and validates each option, including a null path', async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const options = [
      { path: 'C:\\work\\api', label: 'api', count: 12 },
      { path: null, label: 'No recorded folder', count: 3 },
    ];
    invoke.mockResolvedValue(options);

    await expect(listProjects('codex', true)).resolves.toEqual(options);
    expect(invoke).toHaveBeenCalledWith('list_projects', { agent: 'codex', includeSubagents: true });
  });

  describe('demo-path project and subagent filtering', () => {
    // These exercise the branch `!inTauri()` takes, which has to apply the
    // same two filters the backend does so the browser build behaves
    // identically -- nothing above this block calls `searchSessions` or
    // `listProjects` without first stubbing `__TAURI_INTERNALS__`, so nothing
    // else in this suite would notice the demo path silently ignoring either
    // filter.

    it("narrows the demo catalog by project, including the 'unrecorded' arm that actually excludes something", async () => {
      const path = demoSessions[0].project!;
      const matchingCount = demoSessions.filter((session) => session.project === path).length;
      // Two demo sessions share this project (a root and its subagent), so
      // this also proves the path arm is not vacuously matching everything.
      expect(matchingCount).toBeGreaterThan(0);
      expect(matchingCount).toBeLessThan(demoSessions.length);

      const byPath = await searchSessions(undefined, undefined, 0, 200, false, { kind: 'path', path }, true);
      expect(byPath.total).toBe(matchingCount);
      expect(byPath.sessions.every((session) => session.project === path)).toBe(true);

      // No demo fixture has a `project: null` session, so this is a filter
      // that genuinely excludes every row -- proof the filter runs at all,
      // not just that it passes through an empty array unchanged.
      expect(demoSessions.some((session) => session.project === null)).toBe(false);
      const unrecorded = await searchSessions(undefined, undefined, 0, 200, false, { kind: 'unrecorded' }, true);
      expect(unrecorded.total).toBe(0);

      const any = await searchSessions(undefined, undefined, 0, 200, false, { kind: 'any' }, true);
      expect(any.total).toBe(demoSessions.length);
    });

    it('excludes subagent threads from the demo catalog by default and includes them on request', async () => {
      expect(demoSessions.some((session) => session.threadRole.kind === 'subagent')).toBe(true);

      const defaultResult = await searchSessions();
      expect(defaultResult.sessions.every((session) => session.threadRole.kind !== 'subagent')).toBe(true);
      expect(defaultResult.total).toBe(
        demoSessions.filter((session) => session.threadRole.kind !== 'subagent').length,
      );

      const withSubagents = await searchSessions(undefined, undefined, 0, 200, false, undefined, true);
      expect(withSubagents.total).toBe(demoSessions.length);
    });

    it('aggregates the demo catalog into project options sorted by count then label', async () => {
      const options = await listProjects();

      // No demo session omits its project, so the "No recorded folder" entry
      // must be absent rather than padded in at a phantom zero count.
      expect(options.some((option) => option.path === null)).toBe(false);

      const expectedPaths = new Set(
        demoSessions
          .filter((session) => session.threadRole.kind !== 'subagent')
          .map((session) => session.project),
      );
      expect(new Set(options.map((option) => option.path))).toEqual(expectedPaths);

      const total = options.reduce((sum, option) => sum + option.count, 0);
      expect(total).toBe(demoSessions.filter((session) => session.threadRole.kind !== 'subagent').length);

      for (let i = 1; i < options.length; i++) {
        const [prev, curr] = [options[i - 1], options[i]];
        const ordered = prev.count > curr.count ||
          (prev.count === curr.count && prev.label.localeCompare(curr.label) <= 0);
        expect(ordered).toBe(true);
      }

      // includeSubagents flips the same count the search filter uses, so the
      // two must stay in lockstep rather than each having its own notion of
      // which sessions count.
      const withSubagents = await listProjects(undefined, true);
      const totalWithSubagents = withSubagents.reduce((sum, option) => sum + option.count, 0);
      expect(totalWithSubagents).toBe(demoSessions.length);
    });
  });

  // A same-session comparison: both sides name one session, only the turn
  // differs, the shape most of these tests are actually about.
  const leftTarget = { agent: "codex" as const, id: "s", turn: 4 };
  const rightTarget = { agent: "codex" as const, id: "s", turn: 12 };

  it("validates each comparability arm on its own terms", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const base = demoTurnDiff(leftTarget, rightTarget);

    for (const comparability of [
      { kind: "identical", estimator: "o200k_base" },
      { kind: "skewed", left: "chars/2.4", right: "chars/3.1", skew: 0.27 },
      { kind: "incomparable", left: "o200k_base", right: "chars/2.4", reason: "mixed" },
    ]) {
      invoke.mockResolvedValue({ ...base, comparability });
      await expect(getTurnDiff(leftTarget, rightTarget)).resolves.toMatchObject({
        comparability,
      });
    }
  });

  it("rejects an incomparable pair that smuggles in a skew", async () => {
    // The one combination worth a test of its own: `incomparable` means no
    // scale relates the two sides, and a skew of 0 is the *strongest*
    // comparability claim there is. A shape that allowed both would let the
    // absence of a bound arrive looking like a perfect one.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      ...demoTurnDiff(leftTarget, rightTarget),
      comparability: { kind: "incomparable", skew: 0 },
    });

    await expect(getTurnDiff(leftTarget, rightTarget)).rejects.toThrow(
      "invalid response from the turn comparison",
    );
  });

  it("rejects a category row missing the bound that qualifies its delta", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const base = demoTurnDiff(leftTarget, rightTarget);
    const [first, ...rest] = base.categories;
    const { instrumentBound: _dropped, ...withoutBound } = first;
    invoke.mockResolvedValue({ ...base, categories: [withoutBound, ...rest] });

    await expect(getTurnDiff(leftTarget, rightTarget)).rejects.toThrow(
      "invalid response from the turn comparison",
    );
  });

  it("rejects a turn side naming an agent the backend does not know", async () => {
    // `agentLabel` in the UI falls back to "Claude Code" for anything that
    // is not literally "codex" -- an unvalidated agent string here would
    // silently mislabel a side, which is the "two different sessions
    // presented as one" failure `TurnSide.agent` exists to prevent.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const base = demoTurnDiff(leftTarget, rightTarget);
    invoke.mockResolvedValue({ ...base, left: { ...base.left, agent: "gpt-5" } });

    await expect(getTurnDiff(leftTarget, rightTarget)).rejects.toThrow(
      "invalid response from the turn comparison",
    );
  });

  it("holds the demo turn diff to the same contract as the real backend", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue(demoTurnDiff(leftTarget, rightTarget));

    const diff = await getTurnDiff(leftTarget, rightTarget);
    expect(diff.left.turn).toBe(4);
    expect(diff.right.turn).toBe(12);
    expect(diff.left.id).toBe("s");
    expect(diff.right.id).toBe("s");
    // Sorted by magnitude, like the engine sorts them.
    const magnitudes = diff.categories.map((row) => Math.abs(row.delta));
    expect([...magnitudes].sort((a, b) => b - a)).toEqual(magnitudes);
  });

  it("reports a genuine skew for two different Claude Code demo sessions", async () => {
    // The item this suite used to be unable to exercise at all: `skewed` was
    // dead code on the wire until the right side could name a different
    // session. Two different Claude Code demo sessions carry two different
    // fitted ratios (see `DEMO_CLAUDE_RATIOS` in demo.ts), so comparing them
    // is a real cross-session request, not a hand-picked fixture.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const left = { agent: "claude-code" as const, id: "a30cb9e1-f9f4-4a37", turn: 4 };
    const right = { agent: "claude-code" as const, id: "f485150f-0982-4876", turn: 12 };
    invoke.mockResolvedValue(demoTurnDiff(left, right));

    const diff = await getTurnDiff(left, right);
    expect(diff.comparability.kind).toBe("skewed");
    expect(diff.left.id).toBe(left.id);
    expect(diff.right.id).toBe(right.id);
  });

  it("reports incomparable for a Codex turn against a Claude Code turn", async () => {
    // The other previously-unreachable arm: no scale relates a real
    // tokenizer to a fitted heuristic, and mixing agents is the ordinary way
    // that happens now that the two sides can be different sessions.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    const left = { agent: "codex" as const, id: "0198fce2e48a7b12", turn: 4 };
    const right = { agent: "claude-code" as const, id: "a30cb9e1-f9f4-4a37", turn: 12 };
    invoke.mockResolvedValue(demoTurnDiff(left, right));

    const diff = await getTurnDiff(left, right);
    expect(diff.comparability.kind).toBe("incomparable");
    if (diff.comparability.kind === "incomparable") {
      expect(diff.comparability.reason).toContain("Codex records its system prompt");
    }
  });

  it("validates a fitted unlogged-context report and keeps an unknown remainder null", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      kind: "fitted",
      charsPerToken: 3.42,
      pairsUsed: 24,
      dispersion: 1.19,
      unloggedOverhead: 9_400,
      turnsMeasured: 2,
      overCountedTurns: 1,
      stepThreshold: 5_000,
      promptConfidence: "observed",
      remainderConfidence: "derived",
      points: [
        { turn: 1, promptTokens: 12_840, accounted: 3_440, unlogged: 9_400, items: 9 },
        { turn: 2, promptTokens: 15_320, accounted: 16_820, unlogged: null, items: 11 },
      ],
      steps: [{ turn: 18, from: 9_400, to: 15_200, growth: 5_800, nearCompaction: true }],
    });

    const report = await getResidual("claude-code", "s");
    expect(report.kind).toBe("fitted");
    if (report.kind !== "fitted") throw new Error("expected a fitted report");
    // The whole point of the null: an over-counted turn must not arrive as a
    // zero remainder, which would assert a complete inventory of the context.
    expect(report.points[1].unlogged).toBeNull();
    expect(report.steps[0].nearCompaction).toBe(true);
    expect(invoke).toHaveBeenCalledWith("get_residual", { id: "s", agent: "claude-code" });
  });

  it("validates each refusal on its own terms rather than as an empty report", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

    invoke.mockResolvedValue({ kind: "agentNotFitted", agent: "codex" });
    await expect(getResidual("codex", "s")).resolves.toEqual({
      kind: "agentNotFitted",
      agent: "codex",
    });

    invoke.mockResolvedValue({ kind: "insufficientGrowth", turnsWithUsage: 3 });
    await expect(getResidual("claude-code", "s")).resolves.toEqual({
      kind: "insufficientGrowth",
      turnsWithUsage: 3,
    });

    invoke.mockResolvedValue({
      kind: "overCounted",
      charsPerToken: 2.84,
      pairsUsed: 19,
      dispersion: 1.62,
      turnsMeasured: 41,
    });
    const overCounted = await getResidual("claude-code", "s");
    expect(overCounted.kind).toBe("overCounted");
  });

  it("rejects a fitted report that omits the spread qualifying its ratio", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      kind: "fitted",
      charsPerToken: 3.42,
      pairsUsed: 24,
      unloggedOverhead: 9_400,
      turnsMeasured: 1,
      overCountedTurns: 0,
      stepThreshold: 5_000,
      promptConfidence: "observed",
      remainderConfidence: "derived",
      points: [{ turn: 1, promptTokens: 12_840, accounted: 3_440, unlogged: 9_400, items: 9 }],
      steps: [],
    });

    await expect(getResidual("claude-code", "s")).rejects.toThrow(
      "invalid response from the unlogged-context measurement",
    );
  });

  it("rejects an unknown residual outcome rather than treating it as no remainder", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({ kind: "notMeasured", points: [] });

    await expect(getResidual("claude-code", "s")).rejects.toThrow(
      "invalid response from the unlogged-context measurement",
    );
  });

  it("holds every demo residual state to the same contract as the real backend", async () => {
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};

    for (const session of demoSessions) {
      invoke.mockResolvedValue(demoResidual(session.agent, session.id));
      await expect(getResidual(session.agent, session.id)).resolves.toBeTruthy();
    }

    // The fabricated series has to be arithmetic a reader could check: on every
    // measured turn the two columns must add back up to the prompt the demo
    // claims. Three independently typed lists would not.
    const fitted = demoResidual("claude-code", "a30cb9e1-f9f4-4a37");
    expect(fitted.kind).toBe("fitted");
    if (fitted.kind !== "fitted") throw new Error("expected the fitted demo session");
    for (const point of fitted.points) {
      if (point.unlogged == null) {
        expect(point.accounted).toBeGreaterThan(point.promptTokens);
      } else {
        expect(point.accounted + point.unlogged).toBe(point.promptTokens);
      }
    }
    // And each claimed step must be a move the series actually makes, big
    // enough to be worth marking. A marker asserting a level the line never
    // reaches is the same defect as fabricating the fit, one layer down.
    for (const step of fitted.steps) {
      expect(step.to - step.from).toBe(step.growth);
      expect(Math.abs(step.growth)).toBeGreaterThanOrEqual(fitted.stepThreshold);
      const at = fitted.points.find((point) => point.turn === step.turn)?.unlogged;
      expect(at).not.toBeNull();
      expect(Math.abs((at ?? 0) - step.to)).toBeLessThan(0.1 * step.to);
    }
    // The series must drift rather than sit flat between steps — the panel's
    // own caption says one fitted ratio cannot hold a session steady, and a
    // demo that contradicted it would be showing a detector nothing tests.
    const firstLevel = fitted.points
      .filter((point) => point.turn < 18 && point.unlogged != null)
      .map((point) => point.unlogged as number);
    expect(new Set(firstLevel).size).toBeGreaterThan(1);
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

  it("requires the written archive root on startup, not just the read roots", async () => {
    // A missing `archiveRoot` must not pass validation and render `undefined`
    // exactly where this tool's privacy claim names the one directory it
    // writes to.
    (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
    invoke.mockResolvedValue({
      roots: [{ agent: "codex", paths: ["C:\\fixtures\\codex"] }],
      warnings: [],
    });

    await expect(getStartup()).rejects.toThrow(
      "ContextTrace received an invalid response from startup. Refresh and try again.",
    );

    invoke.mockResolvedValue({
      roots: [{ agent: "codex", paths: ["C:\\fixtures\\codex"] }],
      warnings: [],
      archiveRoot: "C:\\fixtures\\archive",
    });
    await expect(getStartup()).resolves.toMatchObject({ archiveRoot: "C:\\fixtures\\archive" });
  });

  describe("archive & export", () => {
    const entry = {
      id: "abc123",
      agent: "codex" as const,
      project: "C:\\work\\demo",
      archivedAt: "2026-01-01T00:00:00Z",
      redaction: "redacted" as const,
      records: 10,
      sourceBytes: 1_000,
      archivedBytes: 1_000,
      redactedRecords: 0,
      redactedValues: 0,
      differsFromSource: false,
    };

    it("validates and forwards the archive listing", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({ root: "C:\\fixtures\\archive", entries: [entry] });

      await expect(archivedSessions()).resolves.toEqual({
        root: "C:\\fixtures\\archive",
        entries: [entry],
      });
      expect(invoke).toHaveBeenCalledWith("archived_sessions");
    });

    it("rejects a redaction value outside the two the domain can produce", async () => {
      // A drifted `"Redacted"` (capitalised) or any other stray string must
      // not pass a bare `typeof === "string"` check -- this is the one field
      // that tells a reader whether a copy still holds credentials, and a
      // loose validator here would render a false "redacted" for a payload
      // whose actual mode is unknown.
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        root: "C:\\fixtures\\archive",
        entries: [{ ...entry, redaction: "Redacted" }],
      });

      await expect(archivedSessions()).rejects.toThrow(
        "ContextTrace received an invalid response from the archive. Refresh and try again.",
      );
    });

    it("rejects an entry missing the flag that says a copy differs from its source", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const { differsFromSource: _dropped, ...withoutFlag } = entry;
      invoke.mockResolvedValue({ root: "C:\\fixtures\\archive", entries: [withoutFlag] });

      await expect(archivedSessions()).rejects.toThrow(
        "ContextTrace received an invalid response from the archive. Refresh and try again.",
      );
    });

    it("archives a session through the requested mode and forwards raw explicitly", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue(entry);

      await expect(archiveSession("codex", "abc123", true)).resolves.toEqual(entry);
      expect(invoke).toHaveBeenCalledWith("archive_session", { id: "abc123", agent: "codex", raw: true });
    });

    it("refuses to archive or export with a stated refusal, not a malformed-response error", async () => {
      // Demo mode has no real session to copy and no real file to write.
      // The rejection must read as a refusal ("nothing to write") rather
      // than as `malformed()`'s "invalid response" wording, which would
      // surface in the UI's error banner as though something had broken.
      delete (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;

      await expect(archiveSession("codex", "abc123", false)).rejects.toThrow(
        "Demo mode has nothing to write.",
      );
      await expect(exportSession("codex", "abc123", false)).rejects.toThrow(
        "Demo mode has nothing to write.",
      );
      expect(invoke).not.toHaveBeenCalled();
    });

    it("validates each ArchiveIntegritySummary kind on its own terms", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const cases: Array<Record<string, unknown>> = [
        { kind: "intact" },
        {
          kind: "sourceChanged",
          recordedDigest: "a",
          currentDigest: "b",
          recordedBytes: 1,
          currentBytes: 2,
        },
        { kind: "sourceGone", archiveMatchesDigest: true },
        { kind: "archiveDamaged", recordedDigest: "a", currentDigest: "b" },
      ];
      for (const integrity of cases) {
        invoke.mockResolvedValue({ integrity, copyIsSound: true, rebuildable: false });
        await expect(verifyArchived("codex", "abc123")).resolves.toMatchObject({ integrity });
      }
    });

    it("rejects a sourceGone verification missing its own boolean rather than defaulting it", async () => {
      // `archiveMatchesDigest` is the one field that distinguishes "the only
      // copy left, and it proves itself" from "the only copy left, and it
      // does not" -- a missing value must not silently read as `false`.
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        integrity: { kind: "sourceGone" },
        copyIsSound: true,
        rebuildable: false,
      });

      await expect(verifyArchived("codex", "abc123")).rejects.toThrow(
        "invalid response from verifying this archived session",
      );
    });

    it("rejects a verification missing copyIsSound or rebuildable rather than re-deriving them", async () => {
      // These are the domain's own judgements
      // (`ArchiveIntegrity::copy_is_sound`/`::rebuildable`), carried across
      // the wire rather than recomputed here -- a missing one must fail
      // loudly, not fall back to a locally guessed answer.
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({ integrity: { kind: "intact" }, rebuildable: false });

      await expect(verifyArchived("codex", "abc123")).rejects.toThrow(
        "invalid response from verifying this archived session",
      );
    });

    it("validates and forwards an export outcome", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const outcome = {
        path: "C:\\fixtures\\archive\\export\\codex\\abc123.ndjson",
        bytes: 4_096,
        records: 12,
        redaction: "secrets" as const,
        redactions: 2,
      };
      invoke.mockResolvedValue(outcome);

      await expect(exportSession("codex", "abc123", true)).resolves.toEqual(outcome);
      expect(invoke).toHaveBeenCalledWith("export_session", {
        id: "abc123",
        agent: "codex",
        redactSecrets: true,
      });
    });

    it("rejects an export redaction value outside none/secrets", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        path: "p",
        bytes: 1,
        records: 1,
        redaction: "all",
        redactions: 0,
      });

      await expect(exportSession("codex", "abc123", false)).rejects.toThrow(
        "invalid response from exporting this session",
      );
    });

    it("holds the demo archive fixtures to the same contract as the real backend", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue(demoArchiveHolding);
      await expect(archivedSessions()).resolves.toEqual(demoArchiveHolding);

      // Every one of the demo holding's entries verifies to a valid,
      // validator-passing shape -- including the four integrity kinds and
      // both readings of `sourceGone`'s own boolean, so all of them are
      // actually reachable by clicking "Verify" on a demo row rather than
      // only existing in demo.ts's own tables.
      const seenKinds = new Set<string>();
      const seenSourceGoneReadings = new Set<boolean>();
      for (const holdingEntry of demoArchiveHolding.entries) {
        const verification = demoArchiveVerification(holdingEntry.agent, holdingEntry.id);
        invoke.mockResolvedValue(verification);
        await expect(verifyArchived(holdingEntry.agent, holdingEntry.id)).resolves.toEqual(
          verification,
        );
        seenKinds.add(verification.integrity.kind);
        if (verification.integrity.kind === "sourceGone") {
          seenSourceGoneReadings.add(verification.integrity.archiveMatchesDigest);
        }
        // A row whose entry says it differs from its source must actually
        // carry replaced values, and vice versa -- the flag and the count it
        // is computed from must agree in the fixture the same way
        // `ArchiveEntry::differs_from_source` requires them to on the real
        // backend.
        expect(holdingEntry.differsFromSource).toBe(holdingEntry.redactedValues > 0);
      }
      expect(seenKinds).toEqual(new Set(["intact", "sourceChanged", "sourceGone", "archiveDamaged"]));
      expect(seenSourceGoneReadings).toEqual(new Set([true, false]));
    });
  });

  describe("cost, instruction files, and temporal ghost regressions", () => {
    it("rejects a cost report with snake_case pricingSource instead of camelCase", async () => {
      // The Rust side shipped returning snake_case while the frontend validator
      // required camelCase, causing "invalid response from cost report". This
      // regression test pins the snake_case shape as the one that must fail.
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        sessionId: "s1",
        pricing_version: "v1",
        pricing_source: "synthetic",
        warning: "demo",
        categories: [],
        total: 100,
        turns: [],
        unpriced: [],
        forecast: null,
      });

      await expect(getCost("codex", "s1", null, null)).rejects.toThrow(
        "ContextTrace received an invalid response from cost report. Refresh and try again.",
      );
    });

    it("accepts a cost report with camelCase pricingVersion and pricingSource", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const expected = demoCost("s1", 0);
      invoke.mockResolvedValue(expected);

      await expect(getCost("codex", "s1", null, null)).resolves.toEqual(expected);
    });

    it("rejects instruction files with snake_case refusalCount instead of camelCase", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        sessionId: "s1",
        projectRoot: "C:\\work",
        comparisons: [],
        refusal_count: 0,
      });

      await expect(getInstructionFiles("codex", "s1")).rejects.toThrow(
        "ContextTrace received an invalid response from instruction-file comparison. Refresh and try again.",
      );
    });

    it("accepts instruction files with camelCase refusalCount", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const expected = demoInstructionFiles("s1");
      invoke.mockResolvedValue(expected);

      await expect(getInstructionFiles("codex", "s1")).resolves.toEqual(expected);
    });

    it("rejects a temporal ghost with snake_case leftTurn instead of camelCase", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue({
        status: "unavailable",
        left_turn: 5,
        right_turn: 10,
        reason: "test reason",
      });

      await expect(getTemporalGhost("codex", "s1", 5, 10)).rejects.toThrow(
        "ContextTrace received an invalid response from the temporal ghost. Refresh and try again.",
      );
    });

    it("accepts a temporal ghost with camelCase leftTurn and rightTurn", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const expected = demoTemporalGhost(5, 10);
      invoke.mockResolvedValue(expected);

      await expect(getTemporalGhost("codex", "s1", 5, 10)).resolves.toEqual(expected);
    });

    it("accepts a temporal ghost's unavailable branch with camelCase fields", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const unavailableGhost = {
        status: "unavailable",
        leftTurn: 5,
        rightTurn: 10,
        reason: "reconstruction unavailable",
      };
      invoke.mockResolvedValue(unavailableGhost);

      await expect(getTemporalGhost("codex", "s1", 5, 10)).resolves.toEqual(unavailableGhost);
    });

    it("rejects a context whose items lack firstSeenTurn or preview field", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const ctx = demoContext();
      // Remove firstSeenTurn from one item to trigger the validation error
      const malformedItems = ctx.items.map((item, index) =>
        index === 0 ? { ...item, firstSeenTurn: undefined } : item
      );
      invoke.mockResolvedValue({ ...ctx, items: malformedItems });

      await expect(getContext("codex", "s1", 32)).rejects.toThrow(
        "ContextTrace received an invalid response from context reconstruction. Refresh and try again.",
      );
    });

    it("rejects a context whose items carry a non-confidence string in confidence field", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const ctx = demoContext();
      const malformedItems = ctx.items.map((item, index) =>
        index === 0 ? { ...item, confidence: "unknown-confidence" } : item
      );
      invoke.mockResolvedValue({ ...ctx, items: malformedItems });

      await expect(getContext("codex", "s1", 32)).rejects.toThrow(
        "ContextTrace received an invalid response from context reconstruction. Refresh and try again.",
      );
    });

    it("validates demoContext() against the real getContext validator", async () => {
      // The demo context must pass the same validation as the real backend,
      // so drifts between types.ts and demo.ts are caught before runtime.
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      invoke.mockResolvedValue(demoContext());

      await expect(getContext("codex", "s1", 32)).resolves.toEqual(demoContext());
    });

    it("holds demoInstructionFiles() to the same contract as the real backend", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const demoFiles = demoInstructionFiles("s1");
      invoke.mockResolvedValue(demoFiles);

      await expect(getInstructionFiles("codex", "s1")).resolves.toEqual(demoFiles);
    });

    it("holds demoCost() to the same contract as the real backend", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const demoCostData = demoCost("s1", 5);
      invoke.mockResolvedValue(demoCostData);

      await expect(getCost("codex", "s1", null, 5)).resolves.toEqual(demoCostData);
    });

    it("holds demoTemporalGhost() to the same contract as the real backend", async () => {
      (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = {};
      const demoGhost = demoTemporalGhost(5, 10);
      invoke.mockResolvedValue(demoGhost);

      await expect(getTemporalGhost("codex", "s1", 5, 10)).resolves.toEqual(demoGhost);
    });
  });
});
