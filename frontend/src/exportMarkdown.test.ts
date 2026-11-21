import { describe, expect, it } from "vitest";
import {
  type ExportInput,
  type ExportTask,
  formatTasksMarkdown,
} from "./exportMarkdown";

const projects = [
  { id: "e", name: "Engine" },
  { id: "p", name: "Personal" },
  { id: "w", name: "Work" },
];
function task(title: string, overrides: Partial<ExportTask> = {}): ExportTask {
  return {
    title,
    description: "",
    scratchpad: "",
    projectId: "e",
    completedAt: null,
    deletedAt: null,
    ...overrides,
  };
}
const input: ExportInput = {
  view: "active",
  query: "",
  projects,
  selectedProjectIds: ["e"],
  tasks: [],
};
const fields = "### Description\n\n_(empty)_\n\n### Scratchpad\n\n_(empty)_";
const priority = "Listed in priority order: task 1 is the most important.";
const options = { formatDay: (iso: string) => iso.slice(0, 10) };

describe("formatTasksMarkdown", () => {
  it("numbers the visible tasks in priority order for a single project filter", () => {
    expect(
      formatTasksMarkdown({
        ...input,
        tasks: [
          task("Last alphabetically"),
          task("First alphabetically"),
          task("Middle"),
        ],
      }),
    ).toBe(
      `# TaskCascade — Active tasks\n\nProjects: Engine · 3 tasks\n\n${priority}\n\n` +
        `## 1. Last alphabetically\n\n${fields}\n\n` +
        `## 2. First alphabetically\n\n${fields}\n\n` +
        `## 3. Middle\n\n${fields}\n`,
    );
  });

  it.each([
    [["w", "p", "e"], "all"],
    [["p", "e"], "Engine, Personal"],
  ] as const)(
    "names filter %j in sidebar order and labels each task",
    (selected, filter) => {
      expect(
        formatTasksMarkdown({
          ...input,
          selectedProjectIds: [...selected],
          tasks: [task("Dentist", { projectId: "p" }), task("Shader")],
        }),
      ).toBe(
        `# TaskCascade — Active tasks\n\nProjects: ${filter} · 2 tasks\n\n${priority}\n\n` +
          `## 1. Dentist\n\nProject: Personal\n\n${fields}\n\n` +
          `## 2. Shader\n\nProject: Engine\n\n${fields}\n`,
      );
    },
  );

  it("uses all when the workspace has only one project and omits the per-task project", () => {
    expect(
      formatTasksMarkdown({
        ...input,
        projects: projects.slice(0, 1),
        tasks: [task("Shader")],
      }),
    ).toBe(
      `# TaskCascade — Active tasks\n\nProjects: all · 1 task\n\n${priority}\n\n## 1. Shader\n\n${fields}\n`,
    );
  });

  it("labels completion dates and projects in the archive without numbering", () => {
    expect(
      formatTasksMarkdown(
        {
          ...input,
          view: "archive",
          selectedProjectIds: ["p", "e"],
          tasks: [task("Shipped", { completedAt: "2026-11-20T09:00:00Z" })],
        },
        options,
      ),
    ).toBe(
      `# TaskCascade — Archive\n\nProjects: Engine, Personal · 1 task\n\n## Shipped\n\nProject: Engine · Completed 2026-11-20\n\n${fields}\n`,
    );
  });

  it("labels the deletion date in Deleted tasks", () => {
    expect(
      formatTasksMarkdown(
        {
          ...input,
          view: "deleted",
          tasks: [task("Removed", { deletedAt: "2026-11-20T09:00:00Z" })],
        },
        options,
      ),
    ).toBe(
      `# TaskCascade — Deleted tasks\n\nProjects: Engine · 1 task\n\n## Removed\n\nDeleted 2026-11-20\n\n${fields}\n`,
    );
  });

  it("labels the cleaned query and mixed search results in visible order", () => {
    expect(
      formatTasksMarkdown(
        {
          ...input,
          view: "search",
          query: "  shader\r\ncache  ",
          tasks: [
            task("Ready"),
            task("Shipped", { completedAt: "2026-11-20T09:00:00Z" }),
          ],
        },
        options,
      ),
    ).toBe(
      `# TaskCascade — Search results for "shader cache"\n\nProjects: Engine · 2 tasks\n\n## Ready\n\nActive\n\n${fields}\n\n## Shipped\n\nCompleted 2026-11-20\n\n${fields}\n`,
    );
  });

  it("normalizes field line endings and trims blank edges, preserving hard breaks", () => {
    expect(
      formatTasksMarkdown({
        ...input,
        tasks: [
          task("Notes", {
            description: " \r\n\rAgenda  \r\nDay one\r\rDay two  \r\n \t\r\n",
            scratchpad: " \n\t ",
          }),
        ],
      }),
    ).toBe(
      `# TaskCascade — Active tasks\n\nProjects: Engine · 1 task\n\n${priority}\n\n## 1. Notes\n\n### Description\n\nAgenda  \nDay one\n\nDay two  \n\n### Scratchpad\n\n_(empty)_\n`,
    );
  });

  it("makes titles single-line, uses an untitled label, and escapes trailing hashes", () => {
    expect(
      formatTasksMarkdown({
        ...input,
        tasks: [task(" \n "), task("  Fix\nissue ##  "), task("#42 crash")],
      }).match(/^## .+$/gm),
    ).toEqual([
      "## 1. (untitled)",
      "## 2. Fix issue \\#\\#",
      "## 3. #42 crash",
    ]);
  });

  it("normalizes project names and labels missing projects", () => {
    expect(
      formatTasksMarkdown({
        ...input,
        projects: [{ id: "e", name: " Engine\rTools " }, ...projects.slice(1)],
        selectedProjectIds: ["e", "p"],
        tasks: [task("Shader"), task("Unknown", { projectId: "missing" })],
      }),
    ).toBe(
      `# TaskCascade — Active tasks\n\nProjects: Engine Tools, Personal · 2 tasks\n\n${priority}\n\n` +
        `## 1. Shader\n\nProject: Engine Tools\n\n${fields}\n\n` +
        `## 2. Unknown\n\nProject: Unknown project\n\n${fields}\n`,
    );
  });

  it("uses a local calendar date and preserves an unparseable date", () => {
    const localNoon = new Date(2026, 10, 20, 12).toISOString();
    const text = formatTasksMarkdown({
      ...input,
      view: "archive",
      tasks: [
        task("Local", { completedAt: localNoon }),
        task("Older", { completedAt: "invalid" }),
      ],
    });
    expect(text).toContain("Completed 2026-11-20");
    expect(text).toContain("Completed invalid");
  });
});
