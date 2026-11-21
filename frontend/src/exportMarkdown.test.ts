import { describe, expect, it } from "vitest";
import { type ExportInput, formatTasksMarkdown } from "./exportMarkdown";

const input: ExportInput = {
  view: "active",
  query: " cache ",
  projects: [{ id: "e", name: "Engine" }],
  selectedProjectIds: ["e"],
  tasks: [
    {
      title: "Second alphabetically",
      description: "Research",
      scratchpad: "A note",
      projectId: "e",
      completedAt: null,
      deletedAt: null,
    },
    {
      title: "First alphabetically",
      description: " \n ",
      scratchpad: "",
      projectId: "e",
      completedAt: null,
      deletedAt: null,
    },
  ],
};

describe("formatTasksMarkdown", () => {
  it.each([
    ["active", "Active tasks"],
    ["archive", "Archive"],
    ["deleted", "Deleted tasks"],
    ["search", 'Search results for "cache"'],
  ] as const)("names the %s view", (view, heading) => {
    expect(formatTasksMarkdown({ ...input, view }).split("\n")[0]).toBe(
      `# TaskCascade — ${heading}`,
    );
  });

  it("keeps the visible order and includes both fields for each task", () => {
    expect(formatTasksMarkdown(input)).toBe(
      "# TaskCascade — Active tasks\n\n" +
        "## Second alphabetically\n\n### Description\n\nResearch\n\n### Scratchpad\n\nA note\n\n" +
        "## First alphabetically\n\n### Description\n\n_(empty)_\n\n### Scratchpad\n\n_(empty)_\n",
    );
  });
});
