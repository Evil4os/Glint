import { ResetCredit } from "./ResetCredit";
import { ClockCounterClockwise, CloudSlash, SignIn, WarningCircle } from "@phosphor-icons/react";
import { memo, useEffect, useRef, useState } from "react";
import type { WidgetAnchor } from "../lib/bridge";
import { clampPercent } from "../lib/format";
import { copy, normalizeLanguage } from "../lib/i18n";
import { shouldActivateAfterPointerGesture, shouldStartDragging } from "../lib/pointer-gesture";
import { resolveQuotaDisplay } from "../lib/quota-display";
import { useQuotaPalette } from "../lib/use-quota-palette";
import { useNativeHalo } from "../lib/use-native-halo";
import { formatWidgetDate, formatWidgetCountdown } from "../lib/widget-format";
import type { ProviderSnapshot, WidgetPreferences } from "../types";

export interface QuotaWidgetProps {
  snapshot: ProviderSnapshot;
  preferences: WidgetPreferences;
  expanded: boolean;
  anchor: WidgetAnchor;
  onActivate: () => void;
  onDrag: () => void | Promise<void>;
  notice?: string | null;
  onResetComplete?: () => void | Promise<void>;
  onResetInteraction?: (active: boolean) => void;

}

interface ActivePointerGesture {
  pointerId: number;
  startedAt: number;
  startX: number;
  startY: number;
  distancePx: number;
  dragStarted: boolean;
}

function UnavailableIcon({ status, expired }: { status: ProviderSnapshot["status"]; expired: boolean }) {
  if (status === "signed_out") return <SignIn weight="duotone" />;
  if (status === "stale" || expired) return <ClockCounterClockwise weight="duotone" />;
  if (status === "unavailable") return <CloudSlash weight="duotone" />;
  return <WarningCircle weight="duotone" />;
}

export const QuotaWidget = memo(function QuotaWidget({ snapshot, preferences, expanded, anchor, onActivate, onDrag, notice = null, onResetComplete, onResetInteraction }: QuotaWidgetProps) {
  const root = useRef<HTMLElement>(null);
  const gestureRef = useRef<ActivePointerGesture | null>(null);
  const [now, setNow] = useState(Date.now);
  const docked = preferences.sidebarDocked;
  const display = resolveQuotaDisplay(snapshot);
  const primary = display.primaryWindow ? clampPercent(display.primaryWindow.remainingPercent) : null;
  const weekly = display.secondaryWeeklyWindow ? clampPercent(display.secondaryWeeklyWindow.remainingPercent) : null;
  const staleExpired = snapshot.status === "stale" && now - new Date(snapshot.updatedAt).getTime() > 30 * 60_000;
  const available = primary !== null && (snapshot.status === "ok" || (snapshot.status === "stale" && !staleExpired));
  const t = copy[normalizeLanguage(preferences.language)];
  const availableLabel = primary === null ? t.unavailableStatus : display.primaryKind === "weekly" ? t.weeklyAvailableLabel(primary) : t.availableLabel(primary);
  const primaryTime = display.primaryKind === "weekly"
    ? formatWidgetDate(display.primaryWindow?.resetsAt ?? null)
    : formatWidgetCountdown(display.primaryWindow?.resetsAt ?? null, now);
  const weeklyDate = formatWidgetDate(display.secondaryWeeklyWindow?.resetsAt ?? null);
  const credits = snapshot.resetCredits;
  const errorLabel = snapshot.status === "signed_out" ? "Sign in" : staleExpired ? "Out of date" : "Unavailable";

  useQuotaPalette(root, available ? primary : null);
  const nativeHalo = useNativeHalo({ percent: available ? primary : null, expanded, sidebarDocked: docked });

  useEffect(() => {
    setNow(Date.now());
    if (!expanded) return;
    const interval = window.setInterval(() => setNow(Date.now()), 15_000);
    return () => window.clearInterval(interval);
  }, [expanded]);

  const handlePointerDown = (event: React.PointerEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    gestureRef.current = { pointerId: event.pointerId, startedAt: performance.now(), startX: event.clientX, startY: event.clientY, distancePx: 0, dragStarted: false };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  };

  const updateDistance = (event: React.PointerEvent<HTMLElement>, gesture: ActivePointerGesture) => {
    gesture.distancePx = Math.max(gesture.distancePx, Math.hypot(event.clientX - gesture.startX, event.clientY - gesture.startY));
  };

  const handlePointerMove = (event: React.PointerEvent<HTMLElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId || gesture.dragStarted) return;
    updateDistance(event, gesture);
    if (docked || !shouldStartDragging(gesture.distancePx)) return;
    gesture.dragStarted = true;
    void Promise.resolve(onDrag()).finally(() => { if (gestureRef.current === gesture) gestureRef.current = null; });
  };

  const finishPointerGesture = (event: React.PointerEvent<HTMLElement>) => {
    const gesture = gestureRef.current;
    if (!gesture || gesture.pointerId !== event.pointerId) return;
    updateDistance(event, gesture);
    gestureRef.current = null;
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
    if (!expanded && shouldActivateAfterPointerGesture({ durationMs: performance.now() - gesture.startedAt, distancePx: gesture.distancePx, dragStarted: gesture.dragStarted })) onActivate();
  };

  return (
    <main
      ref={root}
      className={`quota-widget quota-widget--${anchor}${docked ? " is-docked" : ""}${expanded ? " is-expanded" : ""}${nativeHalo ? " has-native-halo" : ""}`}
      aria-label={available ? availableLabel : snapshot.message ?? t.unavailableStatus}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={finishPointerGesture}
      onPointerCancel={(event) => { if (gestureRef.current?.pointerId === event.pointerId) gestureRef.current = null; }}
    >
      {docked && !expanded ? <button
        type="button"
        className="quota-widget-activate"
        aria-label={`${available ? availableLabel : t.unavailableStatus}，查看额度详情`}
        aria-expanded={false}
        onPointerDown={(event) => event.stopPropagation()}
        onClick={onActivate}
      /> : null}
      <div className="quota-widget-panel">
        <div className="quota-widget-cloud" aria-hidden="true">
          <div className="quota-widget-backdrop" />
        </div>
        <div className="quota-widget-details" aria-hidden={!expanded}>
          {available ? (
            <div className={`widget-data-group${weekly === null ? " is-single" : ""}`}>
              <div className="widget-data-columns">
                <section className="widget-metric" aria-label={display.primaryKind === "weekly" ? "Weekly quota" : "5-hour quota"}>
                  <span className="widget-number">{primary}</span>
                  <span className="widget-time">{primaryTime}</span>
                </section>
                {weekly !== null ? <section className="widget-metric" aria-label="Weekly quota">
                  <span className="widget-number">{weekly}</span>
                  <span className="widget-time">{weeklyDate}</span>
                </section> : null}
              </div>
              <ResetCredit credits={credits} expanded={expanded} onComplete={onResetComplete} onInteraction={onResetInteraction} />
            </div>
          ) : <section className="widget-error" role="status"><UnavailableIcon status={snapshot.status} expired={staleExpired} /><span>{errorLabel}</span></section>}
          {notice ? <p className="widget-notice" role="status">{notice}</p> : null}
        </div>
      </div>
      <div className="quota-widget-badge" aria-hidden="true">
        {available ? <span className="badge-number">{primary}</span> : <UnavailableIcon status={snapshot.status} expired={staleExpired} />}
      </div>
    </main>
  );
});
