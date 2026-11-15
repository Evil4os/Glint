import { describe, expect, it } from "vitest";
import type { UsageWindow } from "../types";
import { resolveQuotaDisplay } from "./quota-display";

const shortWindow: UsageWindow = {
  remainingPercent: 72,
  resetsAt: "2026-07-19T12:00:00.000Z",
  windowSeconds: 18_000,
};

const weeklyWindow: UsageWindow = {
  remainingPercent: 93,
  resetsAt: "2026-07-20T12:00:00.000Z",
  windowSeconds: 604_800,
};

describe("quota display selection", () => {
  it("keeps the 5-hour window primary when both windows exist", () => {
    expect(resolveQuotaDisplay({ shortWindow, weeklyWindow })).toEqual({
      primaryKind: "short",
      primaryWindow: shortWindow,
      secondaryWeeklyWindow: weeklyWindow,
    });
  });

  it("promotes the weekly window only when the 5-hour window is absent", () => {
    expect(resolveQuotaDisplay({ shortWindow: null, weeklyWindow })).toEqual({
      primaryKind: "weekly",
      primaryWindow: weeklyWindow,
      secondaryWeeklyWindow: null,
    });
  });

  it("returns an unavailable display when neither window exists", () => {
    expect(resolveQuotaDisplay({ shortWindow: null, weeklyWindow: null })).toEqual({
      primaryKind: null,
      primaryWindow: null,
      secondaryWeeklyWindow: null,
    });
  });
});
