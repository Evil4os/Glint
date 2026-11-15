/** @vitest-environment jsdom */
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useNativeHalo } from "./use-native-halo";
import { listenHaloReady, publishHaloPresentation, supportsNativeHalo, type HaloPresentation } from "./halo-bridge";

vi.mock("./halo-bridge", () => ({ listenHaloReady: vi.fn(), publishHaloPresentation: vi.fn(), supportsNativeHalo: vi.fn() }));
let ready: (() => void) | undefined;
const unlisten = vi.fn();
const initial: HaloPresentation = { percent: 60, expanded: false, sidebarDocked: true };
beforeEach(() => {
  vi.clearAllMocks();
  ready = undefined;
  vi.mocked(supportsNativeHalo).mockReturnValue(true);
  vi.mocked(publishHaloPresentation).mockResolvedValue();
  vi.mocked(listenHaloReady).mockImplementation(async (handler) => { ready = handler; return unlisten; });
});
afterEach(cleanup);

describe("native halo presentation", () => {
  it("waits for the decorative surface and sends the latest quota and expansion", async () => {
    const { result, rerender } = renderHook((state: HaloPresentation) => useNativeHalo(state), { initialProps: initial });
    expect(result.current).toBe(false);
    rerender({ ...initial, percent: 49, expanded: true });
    act(() => ready?.());
    await waitFor(() => expect(result.current).toBe(true));
    expect(publishHaloPresentation).toHaveBeenLastCalledWith({ percent: 49, expanded: true, sidebarDocked: true });
  });

  it("restores the local cloud when undocking and resumes the native surface when docking", async () => {
    const { result, rerender } = renderHook((state: HaloPresentation) => useNativeHalo(state), { initialProps: initial });
    act(() => ready?.());
    await waitFor(() => expect(result.current).toBe(true));
    rerender({ ...initial, sidebarDocked: false });
    expect(result.current).toBe(false);
    expect(publishHaloPresentation).toHaveBeenLastCalledWith({ ...initial, sidebarDocked: false });
    rerender(initial);
    expect(result.current).toBe(true);
  });

  it("keeps the local fallback if the second surface cannot receive events", async () => {
    vi.mocked(publishHaloPresentation).mockRejectedValue(new Error("window unavailable"));
    const { result } = renderHook(() => useNativeHalo(initial));
    act(() => ready?.());
    await waitFor(() => expect(publishHaloPresentation).toHaveBeenCalled());
    await waitFor(() => expect(result.current).toBe(false));
  });

  it("releases a listener that completes after unmount and does not connect browser previews", async () => {
    let finish: ((value: () => void) => void) | undefined;
    vi.mocked(listenHaloReady).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; }));
    const { unmount } = renderHook(() => useNativeHalo(initial));
    unmount();
    await act(async () => finish?.(unlisten));
    expect(unlisten).toHaveBeenCalledOnce();
    vi.mocked(supportsNativeHalo).mockReturnValue(false);
    const { result } = renderHook(() => useNativeHalo(initial));
    expect(result.current).toBe(false);
    expect(listenHaloReady).toHaveBeenCalledTimes(1);
  });
});
