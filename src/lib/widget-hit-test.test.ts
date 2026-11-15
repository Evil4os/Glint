import { describe, expect, it } from "vitest";
import { isPointInsideWidget } from "./widget-hit-test";

const card = { width: 320, height: 320, scaleFactor: 1, expanded: true, sidebarDocked: true };

describe("widget pointer bounds", () => {
  it("keeps clicks at the original rounded square edge inside the expanded interaction area", () => {
    expect(isPointInsideWidget({ ...card, x: 3, y: 294 })).toBe(true);
    expect(isPointInsideWidget({ ...card, x: 26, y: 317 })).toBe(true);
  });

  it("does not immediately collapse while native resize still reports the small window", () => {
    expect(isPointInsideWidget({ ...card, width: 52, height: 52, x: 26, y: 26 })).toBe(true);
  });

  it("supports physical coordinates on Retina displays", () => {
    expect(isPointInsideWidget({ ...card, width: 640, height: 640, scaleFactor: 2, x: 6, y: 588 })).toBe(true);
  });

  it("still detects leaving the window and excludes unrelated transparent corners", () => {
    expect(isPointInsideWidget({ ...card, x: -1, y: 306 })).toBe(false);
    expect(isPointInsideWidget({ ...card, x: 330, y: 100 })).toBe(false);
    expect(isPointInsideWidget({ ...card, x: 1, y: 1 })).toBe(false);
  });

  it("preserves the floating orb padding", () => {
    const orb = { width: 100, height: 100, scaleFactor: 1, expanded: false, sidebarDocked: false };
    expect(isPointInsideWidget({ ...orb, x: 50, y: 50 })).toBe(true);
    expect(isPointInsideWidget({ ...orb, x: 4, y: 50 })).toBe(false);
  });
});
