import type { Task } from "./api";
import {
  type ExportInput,
  type ExportTask,
  formatTasksMarkdown,
} from "./exportMarkdown";
import { type Fields, fieldsOf, shownFields } from "./saveCoordinator";

export type CopyAvailability = { ok: true } | { ok: false; reason: string };

export function copyAvailability(state: {
  frozen: boolean;
  searchSettled: boolean;
  selectedProjectCount: number;
  taskCount: number;
}): CopyAvailability {
  if (state.frozen) return { ok: false, reason: "Closing…" };
  if (!state.searchSettled)
    return { ok: false, reason: "Waiting for search results" };
  if (!state.selectedProjectCount)
    return { ok: false, reason: "No projects selected" };
  if (!state.taskCount) return { ok: false, reason: "No tasks to copy" };
  return { ok: true };
}

export function toExportTask(
  task: Task,
  draft: Fields | undefined,
): ExportTask {
  const { title, description, scratchpad } = shownFields(
    draft,
    undefined,
    fieldsOf(task),
  );
  const { projectId, completedAt, deletedAt } = task;
  return { title, description, scratchpad, projectId, completedAt, deletedAt };
}

export type ClipboardWriter = (text: string) => Promise<void>;

export function systemClipboard(
  nav: { clipboard?: { writeText?: ClipboardWriter } } | undefined,
): ClipboardWriter {
  return (text) => {
    const clipboard = nav?.clipboard;
    if (typeof clipboard?.writeText !== "function") {
      return Promise.reject(
        new Error("the clipboard is not available in this window"),
      );
    }
    return clipboard.writeText(text);
  };
}

export async function copyVisibleTasks(deps: {
  availability: CopyAvailability;
  input: () => ExportInput;
  write: ClipboardWriter;
  onCopied: (message: string) => void;
  onFailed: (message: string) => void;
}): Promise<"copied" | "unavailable" | "failed"> {
  if (!deps.availability.ok) return "unavailable";
  let count: number;
  try {
    const input = deps.input();
    count = input.tasks.length;
    // Invoke the clipboard before yielding so it can use the current gesture.
    await deps.write(formatTasksMarkdown(input));
  } catch (reason) {
    deps.onFailed(
      `Could not copy to the clipboard: ${reason instanceof Error ? reason.message : String(reason)}`,
    );
    return "failed";
  }
  deps.onCopied(
    `Copied ${count} ${count === 1 ? "task" : "tasks"} as Markdown`,
  );
  return "copied";
}
