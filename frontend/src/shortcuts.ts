import type { View } from "./App";

export type Shortcut =
  | "new-task"
  | "focus-search"
  | "clear-search"
  | "edit-title"
  | "complete"
  | "move-up"
  | "move-down";

export type ShortcutKey = {
  key: string;
  ctrlKey: boolean;
  /** Focus is in a text input, textarea or select that handles the key itself. */
  onEditableField: boolean;
};

export type ShortcutState = {
  frozen: boolean;
  /** A dialog owns the keyboard: its own controls and Escape handle keys. */
  dialogOpen: boolean;
  hasSelection: boolean;
  view: View;
};

/** The window-wide shortcut a keydown triggers, or null to leave the key alone. */
export function shortcutFor(
  event: ShortcutKey,
  state: ShortcutState,
): Shortcut | null {
  if (state.frozen || state.dialogOpen) return null;
  const key = event.key;
  if (event.ctrlKey && key.toLowerCase() === "n") return "new-task";
  if (event.ctrlKey && key.toLowerCase() === "f") return "focus-search";
  if (key === "Escape") return "clear-search";
  if (!state.hasSelection) return null;
  if (
    key === "Enter" &&
    !event.onEditableField &&
    state.view !== "archive" &&
    state.view !== "deleted"
  )
    return "edit-title";
  if (key === "Delete" && !event.onEditableField && state.view === "active")
    return "complete";
  if (event.ctrlKey && key === "ArrowUp" && state.view === "active")
    return "move-up";
  if (event.ctrlKey && key === "ArrowDown" && state.view === "active")
    return "move-down";
  return null;
}
