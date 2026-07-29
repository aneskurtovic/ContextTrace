import { invoke } from "@tauri-apps/api/core";
import type {
  ContextDetail,
  SessionDetail,
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

export function getStartup(): Promise<StartupSummary> {
  if (!inTauri()) return Promise.resolve(demoStartup);
  return invoke("get_startup");
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
  return invoke("list_sessions", {
    agent: agent || null,
    project: project || null,
    limit: 500,
  });
}

export function inspectSession(id: string): Promise<SessionDetail> {
  if (!inTauri()) return Promise.resolve(demoDetail(id));
  return invoke("inspect_session", { id });
}

export function getContext(id: string, turn?: number): Promise<ContextDetail> {
  if (!inTauri()) return Promise.resolve(demoContext(turn));
  return invoke("get_context", { id, turn: turn ?? null });
}
