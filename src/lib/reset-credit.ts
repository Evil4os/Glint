import { isTauri } from "./bridge";

export type ResetOutcome = "reset" | "alreadyRedeemed" | "nothingToReset" | "noCredit" | "notStarted";
export interface ResetResult { outcome: ResetOutcome; refreshed: boolean }
const ATTEMPT_KEY = "quota-float-reset-attempt-v1";

export function pendingResetKey(): string | null {
  return window.localStorage.getItem(ATTEMPT_KEY);
}

export async function reconcilePendingReset(): Promise<boolean> {
  const key = pendingResetKey();
  if (!key || !isTauri()) return false;
  const { invoke } = await import("@tauri-apps/api/core");
  const hasAttempt = await invoke<boolean>("has_pending_reset_credit", { idempotencyKey: key });
  if (!hasAttempt && pendingResetKey() === key) window.localStorage.removeItem(ATTEMPT_KEY);
  return Boolean(pendingResetKey());
}

export async function redeemResetCredit(retry: boolean): Promise<ResetResult> {
  if (!isTauri()) throw new Error("previewOnly");
  const pending = pendingResetKey();
  if (retry && !pending) return { outcome: "notStarted", refreshed: false };
  const key = pending ?? crypto.randomUUID();
  // Persist before dispatch; all uncertain failures, including app restarts, reuse it.
  window.localStorage.setItem(ATTEMPT_KEY, key);
  const { invoke } = await import("@tauri-apps/api/core");
  const result = await invoke<ResetResult>("consume_reset_credit", { idempotencyKey: key, retry: pending !== null });
  if (!Object.hasOwn(resetMessages, result.outcome)) throw new Error("invalidResponse");
  if (pendingResetKey() === key) window.localStorage.removeItem(ATTEMPT_KEY);
  return result;
}

export const resetMessages: Record<ResetOutcome, string> = {
  reset: "已使用 1 张重置卡",
  alreadyRedeemed: "上次重置已成功，未重复扣卡",
  nothingToReset: "当前额度暂不符合重置条件，未扣卡",
  noCredit: "没有可用的重置卡",
  notStarted: "上次重置在提交前结束",
};

export function resetErrorMessage(error: unknown): string {
  const code = error instanceof Error ? error.message : String(error);
  if (code === "cliUnavailable") return "未找到 Codex CLI，请安装后重试";
  if (code === "appServerUnavailable") return "无法启动 Codex 重置服务，请稍后重试";
  if (code === "creditDetailsUnavailable") return "暂时无法读取重置卡详情，请稍后重试";
  if (code === "previewOnly") return "预览中不会使用重置卡，请在桌面组件操作";
  if (code === "attemptStorage" || /storage|quota/i.test(code)) return "无法保存重试信息，请稍后重试";
  return "暂时无法确认重置结果";
}
