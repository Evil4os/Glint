import { describe, expect, it } from "vitest";
import { getQuotaColors, interpolateQuotaColors } from "./quota-colors";

function channelDelta(left: string, right: string) {
  return Math.max(...[1, 3, 5].map((offset) => Math.abs(parseInt(left.slice(offset, offset + 2), 16) - parseInt(right.slice(offset, offset + 2), 16))));
}

describe("shared quota cloud palette", () => {
  it("keeps the approved blue and uses muted red and yellow for lower quota", () => {
    expect(getQuotaColors(100).a).toBe("#b9d9fb");
    expect(getQuotaColors(0).a).toBe("#e8bdb8");
    expect(getQuotaColors(50).a).toBe("#eee0b4");
    expect(getQuotaColors(100).darkB).toBe("#28313c");
    expect(getQuotaColors(-10)).toEqual(getQuotaColors(0));
    expect(getQuotaColors(150)).toEqual(getQuotaColors(100));
  });

  it("changes gently at every percentage without rushing through low quota", () => {
    for (let percent = 1; percent <= 100; percent++) {
      const left = getQuotaColors(percent - 1), right = getQuotaColors(percent);
      for (const key of ["a", "bridge", "b", "darkA", "darkBridge", "darkB"] as const) {
        expect(channelDelta(left[key], right[key])).toBeLessThanOrEqual(3);
      }
    }
    // A high-quota plateau would make most of the displayed range look identical.
    for (let percent = 0; percent < 100; percent += 20) {
      for (const key of ["a", "darkA"] as const) {
        expect(channelDelta(getQuotaColors(percent)[key], getQuotaColors(percent + 20)[key])).toBeGreaterThanOrEqual(5);
      }
    }
  });

  it("keeps unknown quota neutral and temporal interpolation within valid colors", () => {
    expect(getQuotaColors(null)).toEqual(getQuotaColors(NaN));
    expect(getQuotaColors(null)).not.toEqual(getQuotaColors(0));
    const start = getQuotaColors(100), finish = getQuotaColors(0);
    expect(interpolateQuotaColors(start, finish, 0)).toEqual(start);
    expect(interpolateQuotaColors(start, finish, 1)).toEqual(finish);
    for (const color of Object.values(interpolateQuotaColors(start, finish, .5))) expect(color).toMatch(/^#[0-9a-f]{6}$/);
  });
});
