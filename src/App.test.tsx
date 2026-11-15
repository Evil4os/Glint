/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";
import * as bridge from "./lib/bridge";

const desktop = vi.hoisted(() => ({ collapse: null as null | (() => void) }));

vi.mock("./lib/bridge", () => ({
  fetchSnapshots: vi.fn(), isCursorInsideWidget: vi.fn(),
  listenDesktopEvents: vi.fn(), setWidgetExpanded: vi.fn(),
}));

const originalRaf = window.requestAnimationFrame;
const originalMatchMedia = window.matchMedia;
beforeAll(() => {
  window.requestAnimationFrame = (callback) => window.setTimeout(() => callback(performance.now()), 1);
  window.matchMedia = vi.fn().mockReturnValue({ matches: true });
});
afterAll(() => { window.requestAnimationFrame = originalRaf; window.matchMedia = originalMatchMedia; });
afterEach(() => { cleanup(); });

beforeEach(() => {
  vi.clearAllMocks();
  vi.mocked(bridge.setWidgetExpanded).mockResolvedValue();
  vi.mocked(bridge.isCursorInsideWidget).mockResolvedValue(true);
  vi.mocked(bridge.listenDesktopEvents).mockImplementation(async (handlers) => {
    desktop.collapse = handlers.onCollapse;
    return () => { desktop.collapse = null; };
  });
  vi.mocked(bridge.fetchSnapshots).mockResolvedValue([{
    provider: "codex", displayName: "CODEX", plan: "PRO", status: "ok", message: null,
    shortWindow: { remainingPercent: 65, resetsAt: null, windowSeconds: 18_000 },
    weeklyWindow: null, resetCredits: 0, updatedAt: new Date().toISOString(),
  }]);
});

describe("sidebar click-to-open flow", () => {
  it("uses the cache on focus and visibility changes, while startup stays fresh", async () => {
    render(<App />);
    await screen.findByRole("button", { name: /查看额度详情/ });
    expect(bridge.fetchSnapshots).toHaveBeenCalledWith(true);
    vi.mocked(bridge.fetchSnapshots).mockClear();
    await act(async () => { window.dispatchEvent(new Event("focus")); });
    expect(bridge.fetchSnapshots).toHaveBeenLastCalledWith(false);
    await act(async () => { document.dispatchEvent(new Event("visibilitychange")); });
    expect(bridge.fetchSnapshots).toHaveBeenLastCalledWith(false);
  });

  it("opens the actual App on a single disc button click and keeps details visible", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /查看额度详情/ }));
    await waitFor(() => expect(screen.getByRole("main")).toHaveClass("is-expanded"));
    expect(bridge.setWidgetExpanded).toHaveBeenCalledWith(true, "bottom-left");
    expect(screen.getByLabelText("5-hour quota")).toBeVisible();
    expect(screen.queryByRole("button", { name: "设置" })).not.toBeInTheDocument();
    await waitFor(() => expect(bridge.isCursorInsideWidget).toHaveBeenCalledWith(true, true));
    expect(screen.getByRole("main")).toHaveClass("is-expanded");
  });

  it("collapses after the pointer leaves and can open again", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /查看额度详情/ }));
    await waitFor(() => expect(screen.getByRole("main")).toHaveClass("is-expanded"));
    vi.mocked(bridge.isCursorInsideWidget).mockResolvedValue(false);
    await waitFor(() => expect(bridge.setWidgetExpanded).toHaveBeenLastCalledWith(false, "bottom-left"));
    vi.mocked(bridge.isCursorInsideWidget).mockResolvedValue(true);
    fireEvent.click(screen.getByRole("button", { name: /查看额度详情/ }));
    await waitFor(() => expect(screen.getByRole("main")).toHaveClass("is-expanded"));
    expect(bridge.setWidgetExpanded).toHaveBeenLastCalledWith(true, "bottom-left");
  });

  it("can reopen after the native Space transition dismisses details", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: /查看额度详情/ }));
    await waitFor(() => expect(screen.getByRole("main")).toHaveClass("is-expanded"));
    act(() => desktop.collapse?.());
    fireEvent.click(await screen.findByRole("button", { name: /查看额度详情/ }));
    await waitFor(() => expect(screen.getByRole("main")).toHaveClass("is-expanded"));
    expect(vi.mocked(bridge.setWidgetExpanded).mock.calls.filter(([expanded]) => expanded)).toHaveLength(2);
  });
});
