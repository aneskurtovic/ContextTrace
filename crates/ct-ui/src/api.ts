import { invoke } from "@tauri-apps/api/core";
import type {
  ContextDetail,
  SessionDetail,
  SessionPage,
  SessionSummary,
  StartupSummary,
} from "./types";
import {
  demoContext,
  demoDetail,
  demoSessions,
  demoStartup,
} from "./demo";

const inTauri = () =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

type UnknownRecord = Record<string, unknown>;

const agents = new Set(["codex", "claude-code"]);
const confidenceLevels = new Set(["observed", "derived", "estimated"]);

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null;
}

function isStringOrNull(value: unknown) {
  return typeof value === "string" || value === null;
}

function isNumberOrNull(value: unknown) {
  return typeof value === "number" || value === null;
}

function isSession(value: unknown): value is SessionSummary {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.agent === "string" &&
    agents.has(value.agent) &&
    typeof value.path === "string" &&
    typeof value.sizeBytes === "number" &&
    isStringOrNull(value.project) &&
    isStringOrNull(value.startedAt) &&
    isStringOrNull(value.lastActivity)
  );
}

function malformed(command: string): Error {
  return new Error(`ContextTrace received an invalid response from ${command}. Refresh and try again.`);
}

function asStartup(value: unknown): StartupSummary {
  if (
    !isRecord(value) ||
    !Array.isArray(value.roots) ||
    !value.roots.every(
      (root) =>
        isRecord(root) &&
        typeof root.agent === "string" &&
        agents.has(root.agent) &&
        Array.isArray(root.paths) &&
        root.paths.every((path) => typeof path === "string"),
    ) ||
    !Array.isArray(value.warnings) ||
    !value.warnings.every((warning) => typeof warning === "string")
  ) {
    throw malformed("startup");
  }
  return value as unknown as StartupSummary;
}

function asSessions(value: unknown): SessionSummary[] {
  if (!Array.isArray(value) || !value.every(isSession)) {
    throw malformed("session list");
  }
  return value;
}

function asSessionPage(value: unknown): SessionPage {
  if (
    !isRecord(value) ||
    !Array.isArray(value.sessions) ||
    !value.sessions.every(isSession) ||
    typeof value.total !== "number" ||
    !Number.isInteger(value.total) ||
    value.total < 0 ||
    typeof value.offset !== "number" ||
    !Number.isInteger(value.offset) ||
    value.offset < 0 ||
    typeof value.hasMore !== "boolean"
  ) {
    throw malformed("session search");
  }
  return value as unknown as SessionPage;
}

function asDetail(value: unknown): SessionDetail {
  if (
    !isRecord(value) ||
    !isSession(value.session) ||
    !isStringOrNull(value.model) ||
    !isStringOrNull(value.agentVersion) ||
    !isStringOrNull(value.gitBranch) ||
    typeof value.turnCount !== "number" ||
    typeof value.eventCount !== "number" ||
    typeof value.totalOutputTokens !== "number" ||
    !isNumberOrNull(value.peakTurn) ||
    !isNumberOrNull(value.peakPromptTokens) ||
    !isNumberOrNull(value.contextWindow) ||
    typeof value.fidelity !== "number" ||
    typeof value.unrecognisedEvents !== "number" ||
    typeof value.unplacedCompactions !== "number" ||
    !Array.isArray(value.growth) ||
    !value.growth.every(
      (point) =>
        isRecord(point) &&
        typeof point.turn === "number" &&
        (typeof point.promptTokens === "number" || point.promptTokens === null) &&
        (point.compaction === null ||
          (isRecord(point.compaction) &&
            isNumberOrNull(point.compaction.turn) &&
            isNumberOrNull(point.compaction.reclaimed))),
    )
  ) {
    throw malformed("session inspection");
  }
  return value as unknown as SessionDetail;
}

function asContext(value: unknown): ContextDetail {
  if (
    !isRecord(value) ||
    typeof value.turn !== "number" ||
    !isStringOrNull(value.model) ||
    typeof value.totalTokens !== "number" ||
    typeof value.residualTokens !== "number" ||
    typeof value.residualIsMeaningful !== "boolean" ||
    !isNumberOrNull(value.contextWindow) ||
    !isNumberOrNull(value.utilisation) ||
    !isNumberOrNull(value.calibrationScale) ||
    !Array.isArray(value.categories) ||
    !value.categories.every(
      (category) =>
        isRecord(category) &&
        typeof category.category === "string" &&
        typeof category.label === "string" &&
        typeof category.tokens === "number" &&
        typeof category.share === "number" &&
        typeof category.itemCount === "number" &&
        typeof category.confidence === "string" &&
        confidenceLevels.has(category.confidence),
    ) ||
    !Array.isArray(value.contributors) ||
    !value.contributors.every(
      (contributor) =>
        isRecord(contributor) &&
        typeof contributor.id === "string" &&
        typeof contributor.label === "string" &&
        typeof contributor.category === "string" &&
        typeof contributor.source === "string" &&
        typeof contributor.tokens === "number" &&
        typeof contributor.share === "number" &&
        typeof contributor.confidence === "string" &&
        confidenceLevels.has(contributor.confidence),
    )
  ) {
    throw malformed("context reconstruction");
  }
  return value as unknown as ContextDetail;
}

export function getStartup(): Promise<StartupSummary> {
  if (!inTauri()) return Promise.resolve(demoStartup);
  return invoke<unknown>("get_startup").then(asStartup);
}

export function listSessions(
  agent?: string,
  project?: string,
): Promise<SessionSummary[]> {
  if (!inTauri()) {
    const needle = project?.toLocaleLowerCase();
    return Promise.resolve(
      demoSessions.filter(
        (session) =>
          (!agent || session.agent === agent) &&
          (!needle ||
            session.project?.toLocaleLowerCase().includes(needle) ||
            session.id.includes(needle)),
      ),
    );
  }
  return invoke<unknown>("list_sessions", {
    agent: agent || null,
    project: project || null,
    limit: 500,
  }).then(asSessions);
}

export function searchSessions(
  agent?: string,
  query?: string,
  offset = 0,
  limit = 200,
): Promise<SessionPage> {
  if (!inTauri()) {
    const needle = query?.trim().toLocaleLowerCase();
    const matches = demoSessions.filter(
      (session) =>
        (!agent || session.agent === agent) &&
        (!needle ||
          [session.project, session.id, session.path, session.agent]
            .filter(Boolean)
            .some((value) => value!.toLocaleLowerCase().includes(needle))),
    );
    const sessions = matches.slice(offset, offset + limit);
    return Promise.resolve({
      sessions,
      total: matches.length,
      offset,
      hasMore: offset + sessions.length < matches.length,
    });
  }
  return invoke<unknown>("search_sessions", {
    agent: agent || null,
    query: query?.trim() || null,
    offset,
    limit,
  }).then(asSessionPage);
}

export function inspectSession(id: string): Promise<SessionDetail> {
  if (!inTauri()) return Promise.resolve(demoDetail(id));
  return invoke<unknown>("inspect_session", { id }).then(asDetail);
}

export function getContext(id: string, turn?: number): Promise<ContextDetail> {
  if (!inTauri()) return Promise.resolve(demoContext(turn));
  return invoke<unknown>("get_context", { id, turn: turn ?? null }).then(asContext);
}
