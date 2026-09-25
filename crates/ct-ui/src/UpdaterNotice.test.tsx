import { StrictMode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
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

  it("runs the startup check under React StrictMode and leaves a manual check action", async () => {
    render(<StrictMode><UpdaterNotice /></StrictMode>);

    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });

    expect(mocks.invoke).toHaveBeenCalledWith("is_installed_build");
    expect(mocks.check).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole("button", { name: "Check for updates" }));
    await vi.waitFor(() => expect(mocks.check).toHaveBeenCalledTimes(2));
  });

  it("does not run the installer updater from a portable build", async () => {
    mocks.invoke.mockResolvedValue(false);
    render(<StrictMode><UpdaterNotice /></StrictMode>);

    await act(async () => { await vi.advanceTimersByTimeAsync(1800); });

    expect(mocks.check).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "Check for updates" })).toBeNull();
  });
});
