import { describe, expect, it, vi } from "vitest";
import type { Task } from "./api";
import {
  copyAvailability,
  copyVisibleTasks,
  systemClipboard,
  toExportTask,
} from "./copyMarkdown";
import { type ExportInput, formatTasksMarkdown } from "./exportMarkdown";
import {
  initialSearch,
  searchResolved,
  searchSettled,
  searchStarted,
} from "./searchState";

const task: Task = {
  id: "task",
  title: "Stored title",
  description: "Stored description",
  scratchpad: "Stored scratchpad",
  projectId: "project",
  position: 1,
  createdAt: "",
  modifiedAt: "",
  completedAt: "2026-11-20",
  deletedAt: "2026-11-21",
};

const ready = {
  frozen: false,
  searchSettled: true,
  selectedProjectCount: 1,
  taskCount: 2,
};

function setup(taskCount = 2) {
  const input: ExportInput = {
    view: "active",
    query: "",
    projects: [{ id: "project", name: "Project" }],
    selectedProjectIds: ["project"],
    tasks: Array.from({ length: taskCount }, () =>
      toExportTask(task, undefined),
    ),
  };
  return {
    availability: copyAvailability({ ...ready, taskCount }),
    input: vi.fn(() => input),
    write: vi
      .fn<(text: string) => Promise<void>>()
      .mockResolvedValue(undefined),
    onCopied: vi.fn(),
    onFailed: vi.fn(),
  };
}

describe("copyAvailability", () => {
  it.each([
    [
      {
        frozen: true,
        searchSettled: false,
        selectedProjectCount: 0,
        taskCount: 0,
      },
      "Closing…",
    ],
    [
      { searchSettled: false, selectedProjectCount: 0, taskCount: 0 },
      "Waiting for search results",
    ],
    [{ selectedProjectCount: 0, taskCount: 0 }, "No projects selected"],
    [{ taskCount: 0 }, "No tasks to copy"],
  ] as const)("reports the first reason for %o", (patch, reason) => {
    expect(copyAvailability({ ...ready, ...patch })).toEqual({
      ok: false,
      reason,
    });
  });

  it("allows copying a settled, nonempty visible list", () => {
    expect(copyAvailability(ready)).toEqual({ ok: true });
  });
});

describe("toExportTask", () => {
  const stored = {
    title: task.title,
    description: task.description,
    scratchpad: task.scratchpad,
    projectId: task.projectId,
    completedAt: task.completedAt,
    deletedAt: task.deletedAt,
  };

  it("exports stored fields without a draft", () => {
    expect(toExportTask(task, undefined)).toEqual(stored);
  });

  it("uses the draft text and the visible row's project and dates", () => {
    const draft = {
      title: "",
      description: "New description",
      scratchpad: "New scratchpad",
      projectId: "new-project",
    };
    expect(toExportTask(task, draft)).toEqual({
      ...stored,
      ...draft,
      projectId: task.projectId,
    });
  });
});

describe("copyVisibleTasks", () => {
  it.each([
    [1, "task"],
    [2, "tasks"],
  ] as const)(
    "writes %i visible tasks synchronously and confirms on success",
    async (count, label) => {
      const deps = setup(count);
      const outcome = copyVisibleTasks(deps);
      expect(deps.write).toHaveBeenCalledExactlyOnceWith(
        formatTasksMarkdown(deps.input.mock.results[0].value),
      );
      expect(deps.onCopied).not.toHaveBeenCalled();
      await expect(outcome).resolves.toBe("copied");
      expect(deps.onCopied).toHaveBeenCalledExactlyOnceWith(
        `Copied ${count} ${label} as Markdown`,
      );
      expect(deps.onFailed).not.toHaveBeenCalled();
    },
  );

  it("does no work while the repeated search query is pending", async () => {
    const first = searchStarted(initialSearch, "ab");
    const shown = searchResolved(first, first.attempt, [task]);
    const pending = searchStarted(searchStarted(shown, "abc"), "ab");
    const availability = copyAvailability({
      ...ready,
      searchSettled: searchSettled("search", pending),
    });
    expect(availability).toEqual({
      ok: false,
      reason: "Waiting for search results",
    });
    const deps = setup();
    await expect(copyVisibleTasks({ ...deps, availability })).resolves.toBe(
      "unavailable",
    );
    expect(deps.input).not.toHaveBeenCalled();
    expect(deps.write).not.toHaveBeenCalled();
    expect(deps.onFailed).not.toHaveBeenCalled();
    expect(deps.onCopied).not.toHaveBeenCalled();
  });

  it.each([
    ["rejects", (error: Error) => Promise.reject(error)],
    [
      "throws",
      (error: Error) => {
        throw error;
      },
    ],
  ] as const)("reports failure when the writer %s", async (_, fail) => {
    const deps = setup();
    const error = new Error("Document is not focused.");
    deps.write.mockImplementation(() => fail(error));
    await expect(copyVisibleTasks(deps)).resolves.toBe("failed");
    expect(deps.onFailed).toHaveBeenCalledExactlyOnceWith(
      "Could not copy to the clipboard: Document is not focused.",
    );
    expect(deps.onCopied).not.toHaveBeenCalled();
  });
});

describe("systemClipboard", () => {
  it.each([undefined, {}, { clipboard: {} }])(
    "reports an unavailable clipboard for %o",
    async (nav) => {
      await expect(systemClipboard(nav)("text")).rejects.toThrow(
        "the clipboard is not available in this window",
      );
    },
  );

  it("uses the clipboard as the method receiver", async () => {
    const clipboard = {
      writeText: vi
        .fn<(text: string) => Promise<void>>()
        .mockResolvedValue(undefined),
    };
    await systemClipboard({ clipboard })("text");
    expect(clipboard.writeText).toHaveBeenCalledExactlyOnceWith("text");
    expect(clipboard.writeText.mock.contexts).toEqual([clipboard]);
  });
});
