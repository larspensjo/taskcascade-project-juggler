import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, type Task } from "./api";
import {
  type Fields,
  createSaveCoordinator,
  shownFields,
} from "./saveCoordinator";

const fields: Fields = {
  title: "A",
  projectId: "p",
  description: "",
  scratchpad: "",
};
function task(input: Fields = fields, id = "a"): Task {
  return {
    ...input,
    id,
    position: 0,
    createdAt: "",
    modifiedAt: "",
    completedAt: null,
    deletedAt: null,
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function fixture() {
  const send = vi.fn(async (_id: string, input: Fields) => task(input));
  const onSaved = vi.fn();
  return {
    send,
    onSaved,
    c: createSaveCoordinator({ send, onSaved, delayMs: 450 }),
  };
}

describe("save coordinator", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("debounces newest fields and flush cancels the pending timer", async () => {
    const { c, send } = fixture();
    c.edit("a", fields);
    c.edit("a", { ...fields, title: "latest" });
    await vi.advanceTimersByTimeAsync(449);
    expect(send).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(send).toHaveBeenCalledExactlyOnceWith("a", {
      ...fields,
      title: "latest",
    });
    c.edit("a", fields);
    expect(await c.flush("a")).toEqual({ ok: true });
    await vi.advanceTimersByTimeAsync(450);
    expect(send).toHaveBeenCalledTimes(2);
    expect(c.status("a")).toBe("clean");
  });

  it.each([400, 500])(
    "keeps a %i failure, gates actions, and retries exactly once",
    async (status) => {
      const { c, send } = fixture();
      const error = new ApiError(status, "Server rejected the edit");
      send.mockRejectedValue(error);
      c.edit("a", fields);
      expect(await c.flush("a")).toEqual({ ok: false, error });
      expect(c.status("a")).toBe("failed");
      expect(c.draft("a")).toEqual(fields);
      expect(c.error("a")).toBe(error);
      const complete = vi.fn();
      expect((await c.afterSaved("a", complete)).ok).toBe(false);
      expect(complete).not.toHaveBeenCalled();
      const before = send.mock.calls.length;
      send.mockResolvedValue(task());
      expect(await c.flush("a")).toEqual({ ok: true });
      expect(send).toHaveBeenCalledTimes(before + 1);
      expect(c.draft("a")).toBeUndefined();
      expect(c.error("a")).toBeUndefined();
    },
  );

  it("preserves A → B → A drafts and never clears newer fields with a stale response", async () => {
    const { c, send, onSaved } = fixture();
    const first = deferred<Task>();
    const second = deferred<Task>();
    send.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const rev1 = { ...fields, description: "new description" };
    const rev2 = { ...rev1, title: "new title" };
    c.edit("a", rev1);
    const flushed = c.flush("a");
    c.edit("b", { ...fields, title: "B" });
    expect(c.draft("a")).toEqual(rev1);
    c.edit("a", { ...(c.draft("a") as Fields), title: "new title" });
    first.resolve(task(rev1));
    await vi.advanceTimersByTimeAsync(0);
    expect(onSaved).toHaveBeenCalledWith(task(rev1));
    expect(c.draft("a")).toEqual(rev2);
    expect(send.mock.calls.filter(([id]) => id === "a")).toEqual([
      ["a", rev1],
      ["a", rev2],
    ]);
    second.resolve(task(rev2));
    expect(await flushed).toEqual({ ok: true });
    expect(onSaved).toHaveBeenLastCalledWith(task(rev2));
    expect(c.draft("a")).toBeUndefined();
  });

  it("serializes overlapping flushes and coalesces all unsent revisions", async () => {
    const { c, send } = fixture();
    const first = deferred<Task>();
    const second = deferred<Task>();
    send.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    c.edit("a", fields);
    const one = c.flush("a");
    c.edit("a", { ...fields, title: "intermediate" });
    const newest = { ...fields, title: "newest" };
    c.edit("a", newest);
    const two = c.flush("a");
    let finished = false;
    void two.then(() => {
      finished = true;
    });
    expect(send).toHaveBeenCalledTimes(1);
    first.resolve(task());
    await vi.advanceTimersByTimeAsync(0);
    expect(finished).toBe(false);
    expect(send.mock.calls).toEqual([
      ["a", fields],
      ["a", newest],
    ]);
    second.resolve(task(newest));
    expect(await one).toEqual({ ok: true });
    expect(await two).toEqual({ ok: true });
  });

  it("freezes edits without changing the draft, notifications, or timers", async () => {
    const { c, send } = fixture();
    c.edit("a", fields);
    await vi.advanceTimersByTimeAsync(200);
    c.freeze();
    const version = c.version();
    expect(c.edit("a", { ...fields, title: "refused" })).toBe(false);
    expect(c.version()).toBe(version);
    expect(c.draft("a")).toEqual(fields);
    await vi.advanceTimersByTimeAsync(250);
    expect(send).toHaveBeenCalledExactlyOnceWith("a", fields);
    c.unfreeze();
    expect(c.edit("a", fields)).toBe(true);
  });

  it("discard drops drafts and timers without sending", async () => {
    const { c, send } = fixture();
    c.edit("a", fields);
    c.edit("b", fields);
    c.discardAll();
    await vi.advanceTimersByTimeAsync(1000);
    expect(c.draft("a")).toBeUndefined();
    expect(c.draft("b")).toBeUndefined();
    expect(send).not.toHaveBeenCalled();
  });

  it("flushAll starts every task and returns the first failure even if another save is held", async () => {
    const { c, send } = fixture();
    const held = deferred<Task>();
    const error = new Error("Database is locked.");
    send.mockReturnValueOnce(held.promise).mockRejectedValueOnce(error);
    c.edit("a", fields);
    c.edit("b", fields);
    expect(await c.flushAll()).toEqual({ ok: false, error });
    expect(send.mock.calls.map(([id]) => id)).toEqual(["a", "b"]);
    expect(c.draft("a")).toEqual(fields);
    expect(c.draft("b")).toEqual(fields);
    held.resolve(task());
    await c.flush("a");
    expect(c.draft("a")).toBeUndefined();
    expect(c.status("b")).toBe("failed");
  });

  it("stops after a failure until a new edit or explicit retry", async () => {
    const { c, send } = fixture();
    send.mockRejectedValueOnce(new Error("Cannot save."));
    c.edit("a", fields);
    await c.flush("a");
    await vi.advanceTimersByTimeAsync(2000);
    expect(send).toHaveBeenCalledTimes(1);
    c.edit("a", { ...fields, title: "Fixed" });
    await vi.advanceTimersByTimeAsync(450);
    expect(send).toHaveBeenCalledTimes(2);
    expect(c.status("a")).toBe("clean");
  });

  it("sends the newest revision when an older revision fails", async () => {
    const { c, send } = fixture();
    const first = deferred<Task>();
    send.mockReturnValueOnce(first.promise);
    const invalid = { ...fields, title: "" };
    const valid = { ...fields, title: "Valid title" };
    c.edit("a", invalid);
    const flushed = c.flush("a");
    c.edit("a", valid);
    first.reject(new ApiError(400, "Title is required"));
    expect(await flushed).toEqual({ ok: true });
    expect(send.mock.calls).toEqual([
      ["a", invalid],
      ["a", valid],
    ]);
    expect(c.error("a")).toBeUndefined();
    expect(c.status("a")).toBe("clean");
    await vi.advanceTimersByTimeAsync(1000);
    expect(send).toHaveBeenCalledTimes(2);
  });

  it.each([false, true])(
    "discards an orphan edit after a successful complete/delete (in flight: %s)",
    async (inFlight) => {
      const { c, send, onSaved } = fixture();
      const action = deferred<string>();
      const lateSave = deferred<Task>();
      c.edit("a", fields);
      const result = c.afterSaved("a", () => action.promise);
      await vi.advanceTimersByTimeAsync(0);
      if (inFlight) send.mockReturnValueOnce(lateSave.promise);
      c.edit("a", { ...fields, title: "Typed during complete/delete" });
      const lateFlush = inFlight ? c.flush("a") : undefined;
      action.resolve("removed from active tasks");
      expect(await result).toEqual({
        ok: true,
        value: "removed from active tasks",
      });
      c.discard("a");
      c.edit("b", fields);
      expect(c.draft("b")).toEqual(fields);
      if (inFlight) {
        lateSave.reject(new ApiError(404, "Not found"));
        expect(await lateFlush).toEqual({ ok: true });
      }
      send.mockResolvedValue(task(fields, "b"));
      expect(await c.flushAll()).toEqual({ ok: true });
      await vi.advanceTimersByTimeAsync(1000);
      expect(send.mock.calls.filter(([id]) => id === "a")).toHaveLength(
        inFlight ? 2 : 1,
      );
      expect(c.draft("a")).toBeUndefined();
      expect(c.error("a")).toBeUndefined();
      expect(
        onSaved.mock.calls.filter(([saved]) => saved.id === "a"),
      ).toHaveLength(1);
    },
  );

  it.each(["Buy ", " Buy "])(
    "keeps the mounted editor's typed %j after a trimmed confirmation",
    async (title) => {
      let stored = task();
      const c = createSaveCoordinator({
        send: async (_id, input) =>
          task({ ...input, title: input.title.trim() }),
        onSaved: (saved) => {
          stored = saved;
        },
        delayMs: 450,
      });
      const lastEdited = { ...fields, title };
      c.edit("a", lastEdited);
      expect(await c.flush("a")).toEqual({ ok: true });
      expect(c.draft("a")).toBeUndefined();
      expect(stored.title).toBe("Buy");
      const shown = shownFields(c.draft("a"), lastEdited, stored);
      expect(shown.title).toBe(title);
      c.edit("a", { ...shown, title: `${shown.title}milk` });
      expect(await c.flush("a")).toEqual({ ok: true });
      expect(stored.title).toBe("Buy milk");
    },
  );

  it("shows drafts first and uses stored fields when an external change differs", () => {
    const edited = { ...fields, title: " A " };
    const draft = { ...fields, title: "Newer" };
    expect(shownFields(draft, edited, fields)).toBe(draft);
    expect(shownFields(undefined, undefined, fields)).toBe(fields);
    for (const changed of [
      { ...fields, title: "Different" },
      { ...fields, projectId: "other" },
      { ...fields, description: " " },
      { ...fields, scratchpad: " " },
    ]) {
      expect(shownFields(undefined, edited, changed)).toBe(changed);
    }
  });

  it("gates an action until the newest revision is confirmed", async () => {
    const { c, send } = fixture();
    const first = deferred<Task>();
    const second = deferred<Task>();
    send.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const action = vi.fn(async () => "completed");
    c.edit("a", fields);
    const result = c.afterSaved("a", action);
    const newest = { ...fields, title: "Newest" };
    c.edit("a", newest);
    first.resolve(task());
    await vi.advanceTimersByTimeAsync(0);
    expect(action).not.toHaveBeenCalled();
    second.resolve(task(newest));
    expect(await result).toEqual({ ok: true, value: "completed" });
    expect(action).toHaveBeenCalledOnce();
  });
});
