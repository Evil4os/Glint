/** @vitest-environment jsdom */
import { readFileSync } from "node:fs";
import { createElement } from "react";
import { cleanup, render } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, expect, it, vi } from "vitest";
import { QuotaWidget } from "../src/components/QuotaWidget";

// A ready background WebView must not take ownership of the widget's paint.
vi.mock("../src/lib/use-native-halo", () => ({ useNativeHalo: () => true }));

const snapshot = {
  provider: "codex", displayName: "CODEX", plan: null,
  shortWindow: { remainingPercent: 91, resetsAt: null, windowSeconds: 18_000 },
  weeklyWindow: null, resetCredits: 0, resetCreditExpiresAt: [],
  updatedAt: new Date().toISOString(), status: "ok", message: null,
};
let stylesheet;
beforeAll(() => {
  stylesheet = document.createElement("style");
  stylesheet.textContent = readFileSync("src/widget.css", "utf8");
  document.head.append(stylesheet);
});
afterEach(cleanup);
afterAll(() => stylesheet.remove());

it.each([false, true])("keeps the widget's own paint visible with a ready native halo (expanded=%s)", (expanded) => {
  const { container } = render(createElement(QuotaWidget, {
    snapshot, preferences: { sidebarDocked: true, language: "zh-CN" },
    expanded, anchor: "bottom-left", onActivate: vi.fn(), onDrag: vi.fn(),
  }));
  const cloud = container.querySelector(".quota-widget-cloud");
  expect(getComputedStyle(cloud).visibility).toBe("visible");
  expect(container.querySelector("main").style.getPropertyValue("--quota-a")).not.toBe("");
  if (expanded) {
    expect(getComputedStyle(container.querySelector(".quota-widget-backdrop")).opacity).toBe("1");
  }
});
