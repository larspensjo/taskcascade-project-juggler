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

const listMarker = /^(?:[-+*]|[0-9]{1,9}[.)])(?=[ \t]|$)/;
const thematicBreak = /^([-*_])(?:[ \t]*\1){2,}[ \t]*$/;
const atxHeading = /^(#{1,6})(?=[ \t]|$)/;
const closingFence = /^(`{3,}|~{3,})[ \t]*$/;

type Indent = { offset: number; column: number };
type ListItem = { column: number; marker: string };
type Fence = { marker: string; indent: number; column: number };

function readIndent(line: string, fromOffset = 0, fromColumn = 0): Indent {
  let offset = fromOffset;
  let column = fromColumn;
  while (line[offset] === " " || line[offset] === "\t") {
    column += line[offset] === "\t" ? 4 - (column % 4) : 1;
    offset++;
  }
  return { offset, column };
}

function openingFence(text: string): string | undefined {
  return /^(~{3,}|`{3,}(?!`)(?=[^`]*$))/.exec(text)?.[0];
}

function interruptsParagraph(marker: string, text: string): boolean {
  return (
    (marker.length === 1 || marker.slice(0, -1) === "1") &&
    /[^ \t]/.test(text.slice(marker.length))
  );
}

export function embedMarkdown(field: string, shift: number): string {
  const items: ListItem[] = [];
  let fence: Fence | undefined;
  let paragraph = false;

  const lines = field
    .replace(/\r\n?/g, "\n")
    .split("\n")
    .map((line) => {
      let start = readIndent(line);
      if (start.offset === line.length) {
        paragraph = false;
        return line;
      }

      if (fence) {
        if (start.column >= fence.column) {
          const close = closingFence.exec(line.slice(start.offset))?.[1];
          if (
            start.column - fence.column <= 3 &&
            close?.[0] === fence.marker[0] &&
            close.length >= fence.marker.length
          )
            fence = undefined;
          return line;
        }
        // Fenced code has no lazy continuation past its list item's boundary.
        fence = undefined;
      }

      let depth = items.length;
      while (depth > 0 && start.column < items[depth - 1].column) depth--;
      let column = items[depth - 1]?.column ?? 0;
      if (depth < items.length) {
        const text = line.slice(start.offset);
        const marker = listMarker.exec(text)?.[0];
        const startsBlock =
          start.column - column <= 3 &&
          (openingFence(text) ||
            atxHeading.test(text) ||
            thematicBreak.test(text) ||
            text.startsWith(">") ||
            (marker &&
              (marker.at(-1) === items[depth].marker ||
                interruptsParagraph(marker, text))));
        if (paragraph && !startsBlock) return line;
        items.length = depth;
        paragraph = false;
      }

      // A marker line can start several nested items before its first text.
      while (start.column - column <= 3) {
        const text = line.slice(start.offset);
        if (thematicBreak.test(text)) {
          paragraph = false;
          return line;
        }
        const marker = listMarker.exec(text)?.[0];
        if (marker && (!paragraph || interruptsParagraph(marker, text))) {
          const markerEnd = start.column + marker.length;
          start = readIndent(line, start.offset + marker.length, markerEnd);
          const padding = start.column - markerEnd;
          column =
            markerEnd +
            (padding >= 1 && padding <= 4 && start.offset < line.length
              ? padding
              : 1);
          items.push({ column, marker: marker.at(-1) ?? marker });
          paragraph = false;
          if (start.offset === line.length) return line;
          continue;
        }

        const open = openingFence(text);
        if (open) {
          fence = { marker: open, indent: start.column, column };
          paragraph = false;
          return line;
        }
        const heading = atxHeading.exec(text)?.[1];
        if (heading) {
          paragraph = false;
          return (
            line.slice(0, start.offset) +
            "#".repeat(Math.min(6, heading.length + shift)) +
            text.slice(heading.length)
          );
        }
        paragraph = !text.startsWith(">");
        return line;
      }

      // Four columns of indentation can continue a paragraph or start code.
      return line;
    });

  if (fence) lines.push(" ".repeat(fence.indent) + fence.marker);
  return lines.join("\n");
}

function fieldBlock(field: string): string {
  const normalized = field.replace(/\r\n?/g, "\n");
  const lines = normalized.split("\n");
  let start = 0;
  let end = lines.length;
  while (start < end && !lines[start].trim()) start++;
  while (end > start && !lines[end - 1].trim()) end--;
  return start === end
    ? "_(empty)_"
    : embedMarkdown(lines.slice(start, end).join("\n"), 3);
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
