import { describe, expect, it } from "vitest";
import {
  shouldActivateAfterPointerGesture,
  shouldStartDragging,
  type PointerGesture,
} from "./pointer-gesture";

function gesture(overrides: Partial<PointerGesture> = {}): PointerGesture {
  return {
    durationMs: 90,
    distancePx: 0,
    dragStarted: false,
    ...overrides,
  };
}

describe("collapsed widget activation gesture", () => {
  it("expands after a short stationary click", () => {
    expect(shouldActivateAfterPointerGesture(gesture())).toBe(true);
  });

  it("does not expand after a long press", () => {
    expect(shouldActivateAfterPointerGesture(gesture({ durationMs: 600 }))).toBe(false);
  });

  it("starts native dragging as soon as pointer intent passes the threshold", () => {
    expect(shouldStartDragging(3.9)).toBe(false);
    expect(shouldStartDragging(4)).toBe(true);
  });

  it("does not expand once dragging has started", () => {
    expect(shouldActivateAfterPointerGesture(gesture({ distancePx: 18, dragStarted: true }))).toBe(false);
  });

  it("does not expand after unhandled pointer movement", () => {
    expect(shouldActivateAfterPointerGesture(gesture({ distancePx: 5 }))).toBe(false);
  });
});
