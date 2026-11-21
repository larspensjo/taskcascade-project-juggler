export type ExportView = "active" | "archive" | "deleted" | "search";

export type ExportTask = {
  title: string;
  description: string;
  scratchpad: string;
  projectId: string;
  completedAt: string | null;
  deletedAt: string | null;
};

export type ExportInput = {
  view: ExportView;
  query: string;
  projects: { id: string; name: string }[];
  selectedProjectIds: string[];
  tasks: ExportTask[];
};

function viewTitle(view: ExportView, query: string): string {
  switch (view) {
    case "active":
      return "Active tasks";
    case "archive":
      return "Archive";
    case "deleted":
      return "Deleted tasks";
    case "search":
      return `Search results for "${query.trim()}"`;
  }
}

export function formatTasksMarkdown(input: ExportInput): string {
  const blocks = [`# TaskCascade — ${viewTitle(input.view, input.query)}`];
  for (const task of input.tasks) {
    blocks.push(
      `## ${task.title}`,
      "### Description",
      task.description.trim() ? task.description : "_(empty)_",
      "### Scratchpad",
      task.scratchpad.trim() ? task.scratchpad : "_(empty)_",
    );
  }
  return `${blocks.join("\n\n")}\n`;
}
