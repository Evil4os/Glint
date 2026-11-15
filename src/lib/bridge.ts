import type { ProviderSnapshot } from "../types";
import { isPointInsideWidget } from "./widget-hit-test";

export type WidgetAnchor = "top-left" | "top-right" | "bottom-left" | "bottom-right";

const mockSnapshot: ProviderSnapshot = {
  provider: "codex",
  displayName: "CODEX",
  plan: "PRO",
  shortWindow: { remainingPercent: 74, resetsAt: new Date(Date.now() + 78 * 60_000).toISOString(), windowSeconds: 18_000 },
  weeklyWindow: { remainingPercent: 42, resetsAt: new Date(Date.now() + 3.2 * 86_400_000).toISOString(), windowSeconds: 604_800 },
  resetCredits: 1,
  resetCreditExpiresAt: [new Date(Date.now() + 9 * 86_400_000).toISOString()],
  updatedAt: new Date().toISOString(),
  status: "ok",
  message: null,
};

export const isTauri = () => "__TAURI_INTERNALS__" in window;

export async function fetchSnapshots(force = false): Promise<ProviderSnapshot[]> {
  if (!isTauri()) return [mockSnapshot];
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<ProviderSnapshot[]>(force ? "refresh_snapshots" : "get_snapshots");
}

export async function setWidgetExpanded(expanded: boolean, anchor: WidgetAnchor): Promise<void> {
  if (!isTauri()) return;
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("set_widget_expanded", { expanded, anchor });
}

export async function isCursorInsideWidget(expanded: boolean, sidebarDocked = false): Promise<boolean> {
  if (!isTauri()) return true;
  const { cursorPosition, getCurrentWindow } = await import("@tauri-apps/api/window");
  const window = getCurrentWindow();
  const [cursor, position, size, scaleFactor] = await Promise.all([
    cursorPosition(),
    window.outerPosition(),
    window.outerSize(),
    window.scaleFactor(),
  ]);
  return isPointInsideWidget({ x: cursor.x - position.x, y: cursor.y - position.y, width: size.width, height: size.height, scaleFactor, expanded, sidebarDocked });
}

export async function listenDesktopEvents(handlers: {
  onRefresh: () => void;
  onCollapse: () => void;
}): Promise<() => void> {
  if (!isTauri()) return () => undefined;
  const { listen } = await import("@tauri-apps/api/event");
  const unlistenRefresh = await listen("refresh-requested", handlers.onRefresh);
  const unlistenCollapse = await listen("widget-collapsed", handlers.onCollapse);
  return () => { unlistenRefresh(); unlistenCollapse(); };
}
