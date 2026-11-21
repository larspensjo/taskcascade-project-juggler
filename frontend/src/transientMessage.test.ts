import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createTransientMessage } from "./transientMessage";

describe("transient messages", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("shows immediately and dismisses after the duration", () => {
    const show = vi.fn();
    const status = createTransientMessage({ show, durationMs: 2500 });
    status.post("Copied");
    expect(show).toHaveBeenCalledExactlyOnceWith("Copied");
    vi.advanceTimersByTime(2500);
    expect(show).toHaveBeenLastCalledWith(null);
  });

  it("restarts the full duration on a new message", () => {
    const show = vi.fn();
    const status = createTransientMessage({ show, durationMs: 2500 });
    status.post("First");
    vi.advanceTimersByTime(1000);
    status.post("Second");
    vi.advanceTimersByTime(2499);
    expect(show.mock.calls).toEqual([["First"], ["Second"]]);
    vi.advanceTimersByTime(1);
    expect(show).toHaveBeenLastCalledWith(null);
  });

  it("cancels a pending dismissal on dispose", () => {
    const show = vi.fn();
    const status = createTransientMessage({ show, durationMs: 2500 });
    status.post("Copied");
    status.dispose();
    vi.advanceTimersByTime(2500);
    expect(show).toHaveBeenCalledExactlyOnceWith("Copied");
  });
});
