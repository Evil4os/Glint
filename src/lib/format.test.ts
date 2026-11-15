import { describe, expect, it } from "vitest";
import { clampPercent } from "./format";

describe("quota formatting", () => {
  it("clamps untrusted percentages", () => {
    expect(clampPercent(-5)).toBe(0);
    expect(clampPercent(51.6)).toBe(52);
    expect(clampPercent(140)).toBe(100);
  });

});
