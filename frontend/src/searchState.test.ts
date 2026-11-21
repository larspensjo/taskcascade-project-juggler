import { describe, expect, it } from "vitest";
import type { Task } from "./api";
import {
  initialSearch,
  searchResolved,
  searchSettled,
  searchStarted,
} from "./searchState";

const task: Task = {
  id: "task",
  title: "A task",
  description: "",
  scratchpad: "",
  projectId: "project",
  position: 1,
  createdAt: "",
  modifiedAt: "",
  completedAt: null,
  deletedAt: null,
};

describe("search attempts", () => {
  it("settles only when the current attempt resolves", () => {
    const started = searchStarted(initialSearch, "ab");
    expect(searchSettled("search", started)).toBe(false);
    expect(searchSettled("active", started)).toBe(true);
    const resolved = searchResolved(started, started.attempt, [task]);
    expect(searchSettled("search", resolved)).toBe(true);
    expect(resolved.tasks).toEqual([task]);
  });

  it("waits for fresh results when returning to an earlier query", () => {
    const first = searchStarted(initialSearch, "ab");
    const resolved = searchResolved(first, first.attempt, [task]);
    const second = searchStarted(resolved, "abc");
    const latest = searchStarted(second, "ab");
    expect(searchSettled("search", latest)).toBe(false);
    expect(latest.tasks).toEqual([task]);
    expect(searchResolved(latest, second.attempt, [])).toBe(latest);
    const current = searchResolved(latest, latest.attempt, []);
    expect(searchSettled("search", current)).toBe(true);
    expect(searchResolved(current, first.attempt, [task])).toBe(current);
  });

  it("clears and settles immediately for a blank query", () => {
    const started = searchStarted(initialSearch, "ab");
    const resolved = searchResolved(started, started.attempt, [task]);
    const cleared = searchStarted(resolved, " \t");
    expect(searchSettled("search", cleared)).toBe(true);
    expect(cleared.tasks).toEqual([]);
    expect(searchResolved(cleared, started.attempt, [task])).toBe(cleared);
  });
});
