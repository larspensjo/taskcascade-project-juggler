import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { Task } from "./api";
import { runCloseFlow } from "./closeFlow";
import { type Fields, createSaveCoordinator } from "./saveCoordinator";

const fields: Fields = {
  title: "A",
  projectId: "p",
  description: "",
  scratchpad: "",
};
const saved: Task = {
  ...fields,
  id: "a",
  position: 0,
  createdAt: "",
  modifiedAt: "",
  completedAt: null,
  deletedAt: null,
};
function fixture() {
  const send = vi.fn(async (_id: string, _fields: Fields) => saved);
  const coordinator = createSaveCoordinator({
    send,
    onSaved: vi.fn(),
    delayMs: 450,
  });
  const deps = {
    coordinator,
    settle: vi.fn(async () => {}),
    report: vi.fn(async (_o: "saved" | "failed") => {}),
    showFailure: vi.fn(),
    setClosing: vi.fn(),
    timeoutMs: 10_000,
  };
  return { send, deps };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());

it("freezes synchronously before flushing and refuses every edit during close", async () => {
  const { send, deps } = fixture();
  let release!: (task: Task) => void;
  send.mockReturnValue(
    new Promise((resolve) => {
      release = resolve;
    }),
  );
  deps.coordinator.edit("a", fields);
  void deps.coordinator.flush("a");
  const flush = vi.spyOn(deps.coordinator, "flushAll");
  const result = runCloseFlow(deps);
  expect(deps.setClosing).toHaveBeenCalledWith(true);
  expect(deps.setClosing.mock.invocationCallOrder[0]).toBeLessThan(
    flush.mock.invocationCallOrder[0],
  );
  for (const key of ["title", "description", "scratchpad"] as const) {
    expect(deps.coordinator.edit("a", { ...fields, [key]: "refused" })).toBe(
      false,
    );
  }
  release(saved);
  expect(await result).toBe("saved");
  expect(deps.report).toHaveBeenCalledExactlyOnceWith("saved");
  expect(deps.settle).toHaveBeenCalledOnce();
});

it("keeps failed drafts, unfreezes, and retries with exactly one additional send", async () => {
  const { send, deps } = fixture();
  const error = new Error("Title is required.");
  send.mockRejectedValueOnce(error);
  deps.coordinator.edit("a", fields);
  expect(await runCloseFlow(deps)).toBe("failed");
  expect(deps.report).toHaveBeenCalledExactlyOnceWith("failed");
  expect(deps.showFailure).toHaveBeenCalledWith(error);
  expect(deps.setClosing).toHaveBeenLastCalledWith(false);
  expect(deps.coordinator.frozen()).toBe(false);
  expect(deps.coordinator.draft("a")).toEqual(fields);
  expect(await runCloseFlow(deps)).toBe("saved");
  expect(send).toHaveBeenCalledTimes(2);
  expect(deps.report.mock.calls).toEqual([["failed"], ["saved"]]);
});

it("bounds a stuck save and never reports saved after a timeout", async () => {
  const { send, deps } = fixture();
  let release!: (task: Task) => void;
  send.mockReturnValue(
    new Promise((resolve) => {
      release = resolve;
    }),
  );
  deps.coordinator.edit("a", fields);
  const result = runCloseFlow(deps);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(await result).toBe("failed");
  expect(deps.report).toHaveBeenCalledExactlyOnceWith("failed");
  expect(deps.showFailure.mock.calls[0][0].message).toContain("did not finish");
  expect(deps.coordinator.draft("a")).toEqual(fields);
  expect(deps.coordinator.frozen()).toBe(false);
  release(saved);
  await vi.advanceTimersByTimeAsync(0);
  expect(deps.report).toHaveBeenCalledTimes(1);
});

it("waits for other API mutations to settle before reporting saved", async () => {
  const { deps } = fixture();
  let release!: () => void;
  deps.settle.mockReturnValue(
    new Promise((resolve) => {
      release = resolve;
    }),
  );
  const result = runCloseFlow(deps);
  await vi.advanceTimersByTimeAsync(0);
  expect(deps.settle).toHaveBeenCalledOnce();
  expect(deps.report).not.toHaveBeenCalled();
  release();
  expect(await result).toBe("saved");
});

it("bounds settling under the same page deadline", async () => {
  const { deps } = fixture();
  deps.settle.mockReturnValue(new Promise(() => {}));
  const result = runCloseFlow(deps);
  await vi.advanceTimersByTimeAsync(10_000);
  expect(await result).toBe("failed");
  expect(deps.report).toHaveBeenCalledExactlyOnceWith("failed");
  expect(deps.coordinator.frozen()).toBe(false);
});
