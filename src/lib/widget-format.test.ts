import { describe, expect, it } from "vitest";
import { formatWidgetDate, formatWidgetCountdown } from "./widget-format";

describe("minimal widget time labels", () => {
  it("shows hours and remaining minutes without empty units", () => {
    const now = new Date("2026-11-14T12:00:00Z").getTime();
    for (const [minutes, label] of [[1, "1m"], [47, "47m"], [59, "59m"], [60, "1h"], [61, "1h 1m"], [90, "1h 30m"], [297, "4h 57m"], [300, "5h"]] as const) {
      expect(formatWidgetCountdown(new Date(now + minutes * 60_000).toISOString(), now)).toBe(label);
    }
    expect(formatWidgetCountdown(new Date(now + 1).toISOString(), now)).toBe("1m");
    expect(formatWidgetCountdown(new Date(now + 59 * 60_000 + 1).toISOString(), now)).toBe("1h");
    expect(formatWidgetCountdown(new Date(now - 1000).toISOString(), now)).toBe("0m");
  });

  it("shows the weekly reset's local date using the simplest English month", () => {
    expect(formatWidgetDate("2026-11-21T12:00:00")).toBe("Nov 21");
    expect(formatWidgetDate("2026-12-01T12:00:00")).toBe("Dec 1");
  });

  it("does not invent a countdown or date when reset data is missing", () => {
    for (const missing of [null, "invalid"]) {
      expect(formatWidgetCountdown(missing)).toBe("—");
      expect(formatWidgetDate(missing)).toBe("—");
    }
  });
});
