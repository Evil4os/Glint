/** @vitest-environment jsdom */

import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import type { ProviderSnapshot, WidgetPreferences } from "../types";
import { QuotaWidget } from "./QuotaWidget";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const originalPointerEvent = window.PointerEvent;

beforeAll(() => {
  class TestPointerEvent extends MouseEvent {
    pointerId: number;

    constructor(type: string, init: PointerEventInit = {}) {
      super(type, init);
      this.pointerId = init.pointerId ?? 0;
    }
  }
  window.PointerEvent = TestPointerEvent as typeof PointerEvent;
});

afterAll(() => {
  window.PointerEvent = originalPointerEvent;
});

const preferences: WidgetPreferences = {
  sidebarDocked: false,
  language: "zh-CN",
};

const weeklyOnlySnapshot: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "TEAM",
  shortWindow: null,
  weeklyWindow: {
    remainingPercent: 100,
    resetsAt: "2026-07-20T12:00:00.000Z",
    windowSeconds: 604_800,
  },
  resetCredits: 0,
  resetCreditExpiresAt: [],
  updatedAt: "2026-07-19T12:00:00.000Z",
  status: "ok",
  message: null,
};

function renderWidget(snapshot: ProviderSnapshot, expanded: boolean, handlers: { onDrag?: () => void; onActivate?: () => void; sidebarDocked?: boolean; onSidebarDock?: () => void } = {}) {
  const onDrag = handlers.onDrag ?? vi.fn();
  const onActivate = handlers.onActivate ?? vi.fn();
  return {
    ...render(
    <QuotaWidget
      snapshot={snapshot}
      preferences={{ ...preferences, sidebarDocked: handlers.sidebarDocked ?? false }}
      expanded={expanded}
      anchor={handlers.sidebarDocked ? "bottom-left" : "top-left"}
      onDrag={onDrag}
      onActivate={onActivate}
    />,
    ),
    onDrag,
    onActivate,
  };
}

describe("QuotaWidget weekly-only display", () => {
  it("promotes weekly quota in the expanded card without showing a blank 5-hour state", () => {
    const { container } = renderWidget(weeklyOnlySnapshot, true);

    expect(container.querySelector("main")).toHaveAttribute("aria-label", "本周额度剩余 100%");
    expect(screen.getByLabelText("Weekly quota")).toHaveTextContent("100");
    expect(screen.getByText("Jul 20")).toBeInTheDocument();
    expect(container.querySelectorAll(".widget-metric")).toHaveLength(1);
    expect(container.querySelector(".widget-credits")).toHaveTextContent("0resets");
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /重置卡/ })).toBeDisabled();
  });

  it("keeps the weekly percentage visible in the collapsed widget", () => {
    const { container } = renderWidget(weeklyOnlySnapshot, false);
    const collapsedMetric = container.querySelector(".badge-number");

    expect(collapsedMetric).toHaveTextContent("100");
    expect(container.querySelector(".widget-error")).not.toBeInTheDocument();
  });

  it("keeps five-hour quota in the first column with minutes and weekly quota with its date", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-19T15:13:00Z"));
    const { container } = renderWidget({
      ...weeklyOnlySnapshot,
      shortWindow: {
        remainingPercent: 74,
        resetsAt: "2026-07-19T16:00:00.000Z",
        windowSeconds: 18_000,
      },
    }, true);

    expect(screen.getByLabelText("5-hour quota")).toHaveTextContent("74");
    expect(screen.getByText("47m")).toBeInTheDocument();
    expect(screen.getByLabelText("Weekly quota")).toHaveTextContent("100");
    expect(screen.getByText("Jul 20")).toBeInTheDocument();
    expect(container.querySelectorAll(".widget-metric")).toHaveLength(2);
    expect(container.querySelector(".widget-data-columns")?.firstElementChild).toHaveAttribute("aria-label", "5-hour quota");
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(screen.getByText("46m")).toBeInTheDocument();
  });
});

describe("QuotaWidget pointer intent", () => {
  it("starts dragging after pointer movement and never activates", () => {
    const { container, onDrag, onActivate } = renderWidget(weeklyOnlySnapshot, false);
    const widget = container.querySelector("main")!;

    fireEvent.pointerDown(widget, { button: 0, pointerId: 1, clientX: 20, clientY: 20 });
    fireEvent.pointerMove(widget, { pointerId: 1, buttons: 1, clientX: 30, clientY: 20 });
    fireEvent.pointerUp(widget, { button: 0, pointerId: 1, clientX: 30, clientY: 20 });

    expect(onDrag).toHaveBeenCalledTimes(1);
    expect(onActivate).not.toHaveBeenCalled();
  });

  it("activates only after a stationary short click", () => {
    const { container, onDrag, onActivate } = renderWidget(weeklyOnlySnapshot, false);
    const widget = container.querySelector("main")!;

    fireEvent.pointerDown(widget, { button: 0, pointerId: 2, clientX: 20, clientY: 20 });
    fireEvent.pointerUp(widget, { button: 0, pointerId: 2, clientX: 20, clientY: 20 });

    expect(onDrag).not.toHaveBeenCalled();
    expect(onActivate).toHaveBeenCalledTimes(1);
  });

  it("does not activate after a stationary long press", () => {
    vi.useFakeTimers();
    const { container, onDrag, onActivate } = renderWidget(weeklyOnlySnapshot, false);
    const widget = container.querySelector("main")!;

    fireEvent.pointerDown(widget, { button: 0, pointerId: 3, clientX: 20, clientY: 20 });
    vi.advanceTimersByTime(300);
    fireEvent.pointerUp(widget, { button: 0, pointerId: 3, clientX: 20, clientY: 20 });

    expect(onDrag).not.toHaveBeenCalled();
    expect(onActivate).not.toHaveBeenCalled();
  });
});

describe("QuotaWidget sidebar attachment", () => {
  it("exposes a normal button that activates on a single click without pointer gesture timing", () => {
    const { onActivate, onDrag } = renderWidget(weeklyOnlySnapshot, false, { sidebarDocked: true });
    fireEvent.click(screen.getByRole("button", { name: /查看额度详情/ }));
    expect(onActivate).toHaveBeenCalledTimes(1);
    expect(onDrag).not.toHaveBeenCalled();
  });

  it("keeps the badge attached when the pointer moves instead of starting a window drag", () => {
    const { container, onDrag, onActivate } = renderWidget(weeklyOnlySnapshot, false, { sidebarDocked: true });
    const widget = container.querySelector("main")!;

    fireEvent.pointerDown(widget, { button: 0, pointerId: 4, clientX: 18, clientY: 18 });
    fireEvent.pointerMove(widget, { pointerId: 4, buttons: 1, clientX: 38, clientY: 18 });
    fireEvent.pointerUp(widget, { button: 0, pointerId: 4, clientX: 38, clientY: 18 });

    expect(onDrag).not.toHaveBeenCalled();
    expect(onActivate).not.toHaveBeenCalled();
  });

  it("still opens details on a stationary badge click", () => {
    const { container, onActivate } = renderWidget(weeklyOnlySnapshot, false, { sidebarDocked: true });
    const widget = container.querySelector("main")!;

    fireEvent.pointerDown(widget, { button: 0, pointerId: 5, clientX: 18, clientY: 18 });
    fireEvent.pointerUp(widget, { button: 0, pointerId: 5, clientX: 18, clientY: 18 });

    expect(onActivate).toHaveBeenCalledTimes(1);
  });

  it("keeps only the reset action without settings, account title or percentage symbol", () => {
    const { container } = renderWidget({ ...weeklyOnlySnapshot, resetCredits: 3 }, true, { sidebarDocked: true });
    expect(screen.getByRole("button", { name: /3 张重置卡/ })).toBeEnabled();
    expect(screen.queryByText("CODEX")).not.toBeInTheDocument();
    expect(container.querySelector(".widget-credits")).toHaveTextContent("3resets");
    expect(container.querySelector(".quota-widget-details")?.textContent).not.toContain("%");
    expect(container.querySelector(".quota-widget-details")?.textContent).not.toContain("后重置");
  });

  it("keeps unavailable responses separate from a real zero quota", () => {
    const { container } = renderWidget({ ...weeklyOnlySnapshot, status: "unavailable", weeklyWindow: null, resetCredits: null }, true);
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(container.querySelector(".widget-number")).not.toBeInTheDocument();
    expect(container.querySelector(".widget-credits")).not.toBeInTheDocument();
  });
});
