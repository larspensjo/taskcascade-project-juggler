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
      return `Search results for "${singleLine(query)}"`;
  }
}

function singleLine(text: string): string {
  return text.replace(/[\r\n]+/g, " ").trim();
}

function localDay(iso: string): string {
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return iso;
  const year = String(date.getFullYear()).padStart(4, "0");
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function taskStatus(
  view: ExportView,
  task: ExportTask,
  formatDay: (iso: string) => string,
): string | undefined {
  if (view === "deleted" && task.deletedAt)
    return `Deleted ${formatDay(task.deletedAt)}`;
  if ((view === "archive" || view === "search") && task.completedAt)
    return `Completed ${formatDay(task.completedAt)}`;
  if (view === "search") return "Active";
}

function fieldBlock(field: string): string {
  const normalized = field.replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n");
  let start = 0;
  let end = lines.length;
  while (start < end && !lines[start].trim()) start++;
  while (end > start && !lines[end - 1].trim()) end--;
  return start === end ? "_(empty)_" : lines.slice(start, end).join("\n");
}

export function formatTasksMarkdown(
  input: ExportInput,
  { formatDay = localDay }: { formatDay?: (iso: string) => string } = {},
): string {
  const projects = new Map(
    input.projects.map(({ id, name }) => [id, singleLine(name)]),
  );
  const selected = new Set(input.selectedProjectIds);
  const selectedNames = [...projects]
    .filter(([id]) => selected.has(id))
    .map(([, name]) => name);
  const filter =
    selectedNames.length === projects.size ? "all" : selectedNames.join(", ");
  const count = input.tasks.length;
  const blocks = [
    `# TaskCascade — ${viewTitle(input.view, input.query)}`,
    `Projects: ${filter} · ${count} ${count === 1 ? "task" : "tasks"}`,
  ];
  if (input.view === "active")
    blocks.push("Listed in priority order: task 1 is the most important.");

  for (const [index, task] of input.tasks.entries()) {
    const title =
      singleLine(task.title).replace(/#+$/, (hashes) =>
        hashes.replaceAll("#", "\\#"),
      ) || "(untitled)";
    const number = input.view === "active" ? `${index + 1}. ` : "";
    blocks.push(`## ${number}${title}`);

    const meta = [];
    if (selected.size > 1)
      meta.push(
        `Project: ${projects.get(task.projectId) ?? "Unknown project"}`,
      );
    const status = taskStatus(input.view, task, formatDay);
    if (status) meta.push(status);
    if (meta.length) blocks.push(meta.join(" · "));

    blocks.push(
      "### Description",
      fieldBlock(task.description),
      "### Scratchpad",
      fieldBlock(task.scratchpad),
    );
  }
  return `${blocks.join("\n\n")}\n`;
}
