import { useEffect, useRef, useState } from "react";
import { pendingResetKey, reconcilePendingReset, redeemResetCredit, resetErrorMessage, resetMessages } from "../lib/reset-credit";

interface Props {
  credits: number | null;
  expanded: boolean;
  onComplete?: () => void | Promise<void>;
  onInteraction?: (active: boolean) => void;
}

export function ResetCredit({ credits, expanded, onComplete, onInteraction }: Props) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<{ text: string; detail: string } | null>(null);
  const [retry, setRetry] = useState(() => { try { return !!pendingResetKey(); } catch { return false; } });
  const inFlight = useRef(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const confirm = useRef<HTMLButtonElement>(null);
  const restoreFocus = useRef(false);

  useEffect(() => {
    if (!retry) return;
    let active = true;
    void reconcilePendingReset().then((pending) => {
      if (pending || !active || inFlight.current) return;
      setRetry(false);
      setConfirming(false);
    }).catch(() => { /* Keep the retry if the local record could not be checked. */ });
    return () => { active = false; };
  }, [retry]);

  useEffect(() => {
    onInteraction?.(confirming || busy);
    return () => onInteraction?.(false);
  }, [confirming, busy, onInteraction]);
  useEffect(() => { if (!expanded && !busy) setConfirming(false); }, [expanded, busy]);
  useEffect(() => {
    if (confirming) {
      restoreFocus.current = true;
      confirm.current?.focus({ preventScroll: true });
    } else if (!busy && restoreFocus.current) {
      restoreFocus.current = false;
      trigger.current?.focus({ preventScroll: true });
    }
  }, [confirming, busy]);

  const close = () => setConfirming(false);
  const consume = async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setBusy(true);
    setMessage(null);
    try {
      const result = await redeemResetCredit(retry);
      setRetry(false);
      const labels = { reset: "Reset used", alreadyRedeemed: "Already reset", nothingToReset: "Not eligible", noCredit: "No resets", notStarted: "No pending reset" };
      setMessage({ text: labels[result.outcome], detail: resetMessages[result.outcome] + (result.refreshed ? "" : "，额度稍后刷新") });
      close();
    } catch (error) {
      const pending = await reconcilePendingReset().catch(() => {
        try { return Boolean(pendingResetKey()); } catch { return retry; }
      });
      setRetry(pending);
      if (!pending) close();
      const code = error instanceof Error ? error.message : String(error);
      const labels: Record<string, string> = { cliUnavailable: "Codex CLI required", appServerUnavailable: "Reset service unavailable", creditDetailsUnavailable: "Details unavailable", previewOnly: "Preview only", attemptStorage: "Storage unavailable" };
      setMessage({ text: labels[code] ?? (pending ? "Unconfirmed · Retry" : "Reset unavailable"), detail: resetErrorMessage(error) });
    } finally {
      // Refresh on uncertain results too; never locally subtract a card or set quota to 100.
      try { await onComplete?.(); } catch { /* normal polling will retry */ }
      setBusy(false);
      inFlight.current = false;
    }
  };

  return <div className={`widget-reset${expanded && confirming ? " is-confirming" : ""}`} onPointerDown={(e) => e.stopPropagation()} onKeyDown={(e) => {
    if (e.key === "Escape" && !busy) { e.stopPropagation(); close(); }
  }}>
    <button ref={trigger} className="widget-credits" type="button"
      disabled={confirming || busy || (!retry && !(credits !== null && credits > 0))}
      tabIndex={expanded && !confirming ? 0 : -1}
      aria-hidden={expanded && confirming}
      aria-label={retry ? "重试上次重置" : credits === null ? "重置卡暂不可用" : `${credits} 张重置卡，点击使用`}
      aria-expanded={confirming}
      onClick={() => { setConfirming(true); setMessage(null); }}>
      <span>{credits ?? "—"}</span><small>{credits === 1 ? "reset" : "resets"}</small>
    </button>
    {expanded && confirming ? <div className="reset-confirm" role="group" aria-label="使用重置卡">
      <p>{retry ? "Retry reset?" : "Use 1 reset?"}</p>
      <div className="reset-actions">
        <button type="button" aria-label="取消" disabled={busy} onClick={close}>Cancel</button>
        <button ref={confirm} type="button" aria-label={busy ? "正在重置…" : retry ? "重试上次重置" : "使用 1 张重置卡"} disabled={busy} onClick={() => void consume()}>{busy ? "Resetting…" : retry ? "Retry" : "Use"}</button>
      </div>
    </div> : null}
    {message && expanded ? <p className="reset-status" role="status" aria-label={message.detail} title={message.detail}>{message.text}</p> : null}
  </div>;
}
