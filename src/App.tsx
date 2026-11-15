import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { QuotaWidget } from "./components/QuotaWidget";
import { fetchSnapshots, isCursorInsideWidget, listenDesktopEvents, setWidgetExpanded } from "./lib/bridge";
import { copy } from "./lib/i18n";
import { mergeSnapshots } from "./lib/snapshots";
import type { ProviderSnapshot, WidgetPreferences } from "./types";

const WIDGET: WidgetPreferences = { sidebarDocked: true, language: "zh-CN" };
const ANCHOR = "bottom-left" as const;

export default function App() {
  const [snapshots, setSnapshots] = useState<ProviderSnapshot[]>([]);
  const [expanded, setExpanded] = useState(false);
  const [operationError, setOperationError] = useState<string | null>(null);
  const failures = useRef(0);
  const hoverTimer = useRef<number | null>(null);
  const motionRequest = useRef(0);
  const nativeExpanded = useRef(false);
  const pointerInside = useRef(false);
  const refreshInFlight = useRef(false);
  const refreshQueued = useRef(false);
  const resetInteraction = useRef(false);
  const handleResetInteraction = useCallback((active: boolean) => {
    resetInteraction.current = active;
    if (active && hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
  }, []);
  const postExpandRefreshTimer = useRef<number | null>(null);
  const language = "zh-CN" as const;
  const t = copy[language];

  const refresh = useCallback(async (force = false) => {
    if (refreshInFlight.current) {
      if (force) refreshQueued.current = true;
      return;
    }
    refreshInFlight.current = true;
    try {
      const values = await fetchSnapshots(force);
      const hasFailure = values.some((item) => item.status !== "ok");
      if (hasFailure) failures.current += 1;
      else failures.current = 0;
      setSnapshots((current) => mergeSnapshots(current, values));
    } catch {
      failures.current += 1;
      setSnapshots((current) => current.length > 0
        ? current.map((item) => ({ ...item, status: "stale", message: "Refresh failed. Please try again later." }))
        : [{ provider: "codex", displayName: "CODEX", plan: null, shortWindow: null, weeklyWindow: null, resetCredits: null, resetCreditExpiresAt: [], updatedAt: new Date().toISOString(), status: "unavailable", message: "Quota is temporarily unavailable. It will retry automatically." }]);
    } finally {
      refreshInFlight.current = false;
      if (refreshQueued.current) { refreshQueued.current = false; void refresh(true); }
    }
  }, []);

  useEffect(() => {
    void refresh(true);
    return () => {
      if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
      if (postExpandRefreshTimer.current !== null) window.clearTimeout(postExpandRefreshTimer.current);
    };
  }, [refresh]);

  useEffect(() => {
    let cancelled = false;
    let cleanup: () => void = () => {};
    void listenDesktopEvents({
      onRefresh: () => void refresh(true),
      onCollapse: () => {
        motionRequest.current += 1;
        nativeExpanded.current = false;
        pointerInside.current = false;
        if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
        if (postExpandRefreshTimer.current !== null) window.clearTimeout(postExpandRefreshTimer.current);
        setExpanded(false);
      },
    }).then((value) => {
      if (cancelled) value(); else cleanup = value;
    }).catch(() => setOperationError("Desktop event listener failed to start."));
    return () => { cancelled = true; cleanup(); };
  }, [refresh]);

  const refreshMs = useMemo(() => {
    return failures.current === 0
      ? 60_000
      : Math.min(30 * 60_000, 30_000 * 2 ** (failures.current - 1));
  }, [snapshots]);

  useEffect(() => {
    const id = window.setInterval(() => void refresh(), refreshMs);
    return () => window.clearInterval(id);
  }, [refresh, refreshMs]);

  useEffect(() => {
    // Focus/visibility events often arrive together. Let the native 30-second
    // cache serve them; an explicit click still requests a fresh snapshot.
    const refreshWhenActive = () => { if (document.visibilityState === "visible") void refresh(); };
    window.addEventListener("focus", refreshWhenActive);
    document.addEventListener("visibilitychange", refreshWhenActive);
    return () => {
      window.removeEventListener("focus", refreshWhenActive);
      document.removeEventListener("visibilitychange", refreshWhenActive);
    };
  }, [refresh]);

  const current = snapshots[0];

  const transitionWidget = useCallback(async (value: boolean) => {
    const request = ++motionRequest.current;
    if (value) {
      if (!nativeExpanded.current) {
        await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
        if (request !== motionRequest.current) return;
        try {
          await setWidgetExpanded(true, ANCHOR);
          nativeExpanded.current = true;
        } catch {
          if (request !== motionRequest.current) return;
          setOperationError("Widget expand failed.");
          return;
        }
        await new Promise<void>((resolve) => window.requestAnimationFrame(() => resolve()));
        if (request !== motionRequest.current) return;
      }
      setExpanded(true);
      return;
    }

    setExpanded(false);
    const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    if (!reduceMotion) await new Promise<void>((resolve) => window.setTimeout(resolve, 310));
    if (request !== motionRequest.current || !nativeExpanded.current) return;
    try {
      await setWidgetExpanded(false, ANCHOR);
      nativeExpanded.current = false;
    } catch {
      if (request !== motionRequest.current) return;
      setExpanded(true);
      setOperationError("Widget collapse failed.");
    }
  }, []);

  const handlePointerPresence = useCallback((value: boolean) => {
    if (resetInteraction.current) return;
    pointerInside.current = value;
    if (hoverTimer.current !== null) window.clearTimeout(hoverTimer.current);
    if (value) return;
    hoverTimer.current = window.setTimeout(() => {
      hoverTimer.current = null;
      void transitionWidget(false);
    }, 180);
  }, [transitionWidget]);

  const handleActivate = useCallback(() => {
    if (expanded) return;
    pointerInside.current = true;
    if (hoverTimer.current !== null) {
      window.clearTimeout(hoverTimer.current);
      hoverTimer.current = null;
    }
    void transitionWidget(true).then(() => {
      if (postExpandRefreshTimer.current !== null) window.clearTimeout(postExpandRefreshTimer.current);
      postExpandRefreshTimer.current = window.setTimeout(() => {
        postExpandRefreshTimer.current = null;
        void refresh(true);
      }, 500);
    });
  }, [expanded, refresh, transitionWidget]);

  useEffect(() => {
    if (!expanded) return;
    let disposed = false;
    let checking = false;
    const checkPointer = async () => {
      if (disposed || checking) return;
      checking = true;
      try {
        const inside = await isCursorInsideWidget(true, true);
        if (!disposed && inside !== pointerInside.current) handlePointerPresence(inside);
      } catch {
        // DOM enter/leave remains the fallback when cursor inspection is unavailable.
      } finally {
        checking = false;
      }
    };
    void checkPointer();
    const id = window.setInterval(() => void checkPointer(), 75);
    return () => {
      disposed = true;
      window.clearInterval(id);
    };
  }, [expanded, handlePointerPresence]);

  if (!current) return <div className="widget-loading" aria-label={t.loadingQuota}><span /></div>;

  return (
    <QuotaWidget
      snapshot={current}
      preferences={WIDGET}
      expanded={expanded}
      anchor={ANCHOR}
      onDrag={() => {}}
      onActivate={handleActivate}
      onResetComplete={() => refresh(true)}
      onResetInteraction={handleResetInteraction}
      notice={operationError}
    />
  );
}
