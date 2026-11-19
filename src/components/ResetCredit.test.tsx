/** @vitest-environment jsdom */
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
const { redeem, pending, reconcile } = vi.hoisted(() => ({ redeem: vi.fn(), pending: vi.fn(), reconcile: vi.fn() }));
vi.mock("../lib/reset-credit", async (original) => ({
  ...await original<typeof import("../lib/reset-credit")>(),
  redeemResetCredit: redeem,
  pendingResetKey: pending,
  reconcilePendingReset: reconcile,
}));
import { ResetCredit } from "./ResetCredit";
afterEach(cleanup);
beforeEach(() => { redeem.mockReset(); pending.mockReset(); reconcile.mockReset().mockResolvedValue(false); });
it("requires confirmation and cancelling never redeems", () => {
  render(<ResetCredit credits={3} expanded />);
  fireEvent.click(screen.getByRole("button", { name: /3 张/ }));
  expect(redeem).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "取消" }));
  expect(redeem).not.toHaveBeenCalled();
  expect(screen.queryByRole("group")).not.toBeInTheDocument();
});
it("locks interaction while pending, blocks repeated clicks and refreshes after success", async () => {
  let resolve!: (v: unknown) => void;
  redeem.mockImplementation(() => new Promise(r => { resolve = r; }));
  const onComplete = vi.fn(), onInteraction = vi.fn();
  render(<ResetCredit credits={3} expanded onComplete={onComplete} onInteraction={onInteraction} />);
  fireEvent.click(screen.getByRole("button", { name: /3 张/ }));
  const button = screen.getByRole("button", { name: "使用 1 张重置卡" });
  fireEvent.click(button); fireEvent.click(button);
  expect(redeem).toHaveBeenCalledTimes(1);
  expect(onInteraction).toHaveBeenLastCalledWith(true);
  expect(screen.getByRole("button", { name: "正在重置…" })).toBeDisabled();
  await act(async () => { resolve({ outcome: "reset", refreshed: true }); });
  expect(onComplete).toHaveBeenCalledTimes(1);
  expect(screen.getByRole("status")).toHaveTextContent("Reset used");
  expect(onInteraction).toHaveBeenLastCalledWith(false);
});
it("allows a previous uncertain attempt to retry even when available credits reach zero", async () => {
  pending.mockReturnValue("previous-key");
  reconcile.mockResolvedValue(true);
  redeem.mockResolvedValue({ outcome: "alreadyRedeemed", refreshed: true });
  render(<ResetCredit credits={0} expanded />);
  fireEvent.click(screen.getByRole("button", { name: "重试上次重置" }));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "重试上次重置" })); });
  expect(screen.getByRole("status")).toHaveTextContent("Already reset");
});
it("offers retry after a failure and does not change displayed credit count", async () => {
  redeem.mockRejectedValue("connectionLost"); const onComplete = vi.fn();
  reconcile.mockResolvedValue(true);
  render(<ResetCredit credits={3} expanded onComplete={onComplete} />);
  fireEvent.click(screen.getByRole("button", { name: /3 张/ }));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "使用 1 张重置卡" })); });
  expect(screen.getByRole("status")).toHaveTextContent("Unconfirmed · Retry");
  expect(screen.getAllByRole("button", { name: "重试上次重置" })).toHaveLength(1);
  expect(onComplete).toHaveBeenCalledTimes(1);
});
it("restores the card count when a legacy retry has no submitted attempt", async () => {
  pending.mockReturnValue("unsubmitted-key");
  render(<ResetCredit credits={2} expanded />);
  fireEvent.click(screen.getByRole("button", { name: "重试上次重置" }));
  await waitFor(() => expect(screen.queryByRole("group")).not.toBeInTheDocument());
  fireEvent.click(screen.getByRole("button", { name: /2 张/ }));
  expect(screen.getByRole("group")).toHaveTextContent("Use 1 reset?");
  expect(redeem).not.toHaveBeenCalled();
});
it("shows the startup error and closes confirmation when no reset was sent", async () => {
  redeem.mockRejectedValue("appServerUnavailable");
  render(<ResetCredit credits={2} expanded />);
  fireEvent.click(screen.getByRole("button", { name: /2 张/ }));
  await act(async () => { fireEvent.click(screen.getByRole("button", { name: "使用 1 张重置卡" })); });
  expect(screen.getByRole("status")).toHaveTextContent("Reset service unavailable");
  expect(screen.queryByRole("group")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /2 张/ })).toBeEnabled();
});
