import { expect, it } from "vitest";
import { type ShortcutState, shortcutFor } from "./shortcuts";

const browsing: ShortcutState = {
  frozen: false,
  dialogOpen: false,
  hasSelection: true,
  view: "active",
};
const key = (key: string, ctrlKey = false, onEditableField = false) => ({
  key,
  ctrlKey,
  onEditableField,
});

it("leaves Enter on a focused dialog button to the button while a task is selected", () => {
  expect(shortcutFor(key("Enter"), { ...browsing, dialogOpen: true })).toBe(
    null,
  );
});

it("takes no global shortcut at all while a dialog is open", () => {
  const dialog = { ...browsing, dialogOpen: true };
  for (const event of [
    key("Enter"),
    key("Delete"),
    key("Escape"),
    key("n", true),
    key("f", true),
    key("ArrowUp", true),
    key("ArrowDown", true),
  ]) {
    expect(shortcutFor(event, dialog)).toBe(null);
  }
});

it("maps the list shortcuts when no dialog is open", () => {
  expect(shortcutFor(key("Enter"), browsing)).toBe("edit-title");
  expect(shortcutFor(key("Delete"), browsing)).toBe("complete");
  expect(shortcutFor(key("Escape"), browsing)).toBe("clear-search");
  expect(shortcutFor(key("N", true), browsing)).toBe("new-task");
  expect(shortcutFor(key("f", true), browsing)).toBe("focus-search");
  expect(shortcutFor(key("ArrowUp", true), browsing)).toBe("move-up");
  expect(shortcutFor(key("ArrowDown", true), browsing)).toBe("move-down");
});

it("leaves Enter and Delete to editable fields", () => {
  expect(shortcutFor(key("Enter", false, true), browsing)).toBe(null);
  expect(shortcutFor(key("Delete", false, true), browsing)).toBe(null);
});

it("takes no shortcut while closing freezes the window", () => {
  expect(shortcutFor(key("n", true), { ...browsing, frozen: true })).toBe(null);
});

it("only edits or completes where the selected task is active", () => {
  expect(shortcutFor(key("Enter"), { ...browsing, view: "archive" })).toBe(
    null,
  );
  expect(shortcutFor(key("Enter"), { ...browsing, view: "search" })).toBe(
    "edit-title",
  );
  expect(shortcutFor(key("Delete"), { ...browsing, view: "search" })).toBe(
    null,
  );
  expect(shortcutFor(key("Enter"), { ...browsing, hasSelection: false })).toBe(
    null,
  );
});
