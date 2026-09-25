import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import UpdaterNotice from "./UpdaterNotice";

const mocks = vi.hoisted(() => ({
  check: vi.fn(),
  invoke: vi.fn(),
  isTauri: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mocks.invoke,
  isTauri: mocks.isTauri,
}));
vi.mock("@tauri-apps/plugin-updater", () => ({ check: mocks.check }));

describe("desktop updater notice", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mocks.isTauri.mockReset();
    mocks.invoke.mockReset();
    mocks.check.mockReset();
    mocks.isTauri.mockReturnValue(true);
    mocks.invoke.mockResolvedValue(true);
    mocks.check.mockResolvedValue(null);
  });

  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  it("checks on startup under React StrictMode without a persistent up-to-date banner", async () => {
    render(<StrictMode><UpdaterNotice /></StrictMode>);

    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });

    expect(mocks.invoke).toHaveBeenCalledWith("is_installed_build");
    expect(mocks.check).toHaveBeenCalledTimes(1);
    expect(screen.queryByText("ContextTrace is up to date.")).toBeNull();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("silently ignores startup feed failures", async () => {
    mocks.check.mockRejectedValue(new Error("feed unavailable"));
    render(<StrictMode><UpdaterNotice /></StrictMode>);

    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });

    expect(screen.queryByRole("status")).toBeNull();
  });

  it("shows an available update", async () => {
    mocks.check.mockResolvedValue({ version: "0.1.3", close: vi.fn(), downloadAndInstall: vi.fn() });
    render(<StrictMode><UpdaterNotice /></StrictMode>);

    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });

    expect(screen.getByText("ContextTrace 0.1.3 is ready")).not.toBeNull();
    expect(screen.getByRole("button", { name: "Install update" })).not.toBeNull();
  });

  it("does not run the installer updater from a portable build", async () => {
    mocks.invoke.mockResolvedValue(false);
    render(<StrictMode><UpdaterNotice /></StrictMode>);

    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });

    expect(mocks.check).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
  });
});
