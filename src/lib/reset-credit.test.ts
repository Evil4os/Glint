/** @vitest-environment jsdom */
import { beforeEach, describe, expect, it, vi } from "vitest";
const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("./bridge", () => ({ isTauri: () => true }));
import { pendingResetKey, reconcilePendingReset, redeemResetCredit } from "./reset-credit";

beforeEach(() => {
  const data = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => data.set(key, value),
    removeItem: (key: string) => data.delete(key),
  });
  invoke.mockReset();
});
describe("reset credit request identity", () => {
  it("reuses a persisted key after an uncertain failure and clears it only for a known outcome", async () => {
    invoke.mockRejectedValueOnce("connectionLost").mockResolvedValueOnce({ outcome: "alreadyRedeemed", refreshed: true });
    await expect(redeemResetCredit(false)).rejects.toBe("connectionLost");
    const key = pendingResetKey(); expect(key).toBeTruthy();
    await expect(redeemResetCredit(true)).resolves.toMatchObject({ outcome: "alreadyRedeemed" });
    expect(invoke.mock.calls[0][1]).toEqual({ idempotencyKey: key, retry: false });
    expect(invoke.mock.calls[1][1]).toEqual({ idempotencyKey: key, retry: true });
    expect(pendingResetKey()).toBeNull();
  });
  it("retains the request on an unrecognized server response", async () => {
    invoke.mockResolvedValueOnce({ outcome: "unknown" });
    await expect(redeemResetCredit(false)).rejects.toThrow("invalidResponse");
    expect(pendingResetKey()).toBeTruthy();
  });
  it("clears an unsubmitted key without turning a stale retry into a new reset", async () => {
    invoke.mockRejectedValueOnce("appServerUnavailable").mockResolvedValueOnce(false);
    await expect(redeemResetCredit(false)).rejects.toBe("appServerUnavailable");
    const key = pendingResetKey();
    await expect(reconcilePendingReset()).resolves.toBe(false);
    expect(invoke).toHaveBeenLastCalledWith("has_pending_reset_credit", { idempotencyKey: key });
    expect(pendingResetKey()).toBeNull();
    await expect(redeemResetCredit(true)).resolves.toMatchObject({ outcome: "notStarted" });
    expect(invoke).toHaveBeenCalledTimes(2);
  });
});
