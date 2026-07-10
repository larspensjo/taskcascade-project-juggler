```markdown
# Project Coordination Tool — MVP Requirements

## Purpose

A lightweight personal project coordination tool for a single developer.

The application is intentionally minimal. Its primary goal is to keep track of what to work on next while allowing rapid reprioritization and continuous note-taking.

It is **not** intended to compete with issue trackers such as Jira, GitHub Issues or Trello.

---

# Design Principles

- Simple before powerful.
- Minimize user interaction.
- Zero configuration.
- Keyboard-first.
- Immediate persistence.
- One active ordered task list.
- No explicit priorities.

---

# Core Concepts

## Task

A task represents an idea or piece of work.

Each task contains:

- Title
- Description (Markdown)
- Scratchpad (Markdown)
- Project tag
- Created timestamp
- Modified timestamp
- Completed timestamp (archive only)

---

## Ordered Task List

The application maintains a single ordered list.

Properties:

- Highest priority is at the top.
- Lowest priority is at the bottom.
- Priority is defined solely by list position.
- There are no numeric priorities.

---

## Projects

Each task belongs to exactly one project.

Typical use case:

- Engine
- MergeScribe
- Logonaut
- Personal

The project list should be user configurable.

---

# Functional Requirements

## Create Task

The user can create a task.

Required fields:

- Title
- Project

Optional:

- Description

New tasks are inserted at the top of the list.

---

## Edit Task

The user can edit:

- Title
- Description
- Scratchpad
- Project

Changes are saved automatically.

---

## Delete / Complete Task

Completing a task removes it from the active list.

Completed tasks are moved to an archive.

The archive preserves:

- Title
- Description
- Scratchpad
- Project
- All timestamps

---

## Reorder Tasks

Tasks can be reordered.

Methods:

- Drag and drop
- Keyboard shortcuts

Reordering changes priority immediately.

---

## Scratchpad

Each task contains a scratchpad.

Purpose:

- Temporary notes
- Investigation
- Code snippets
- Future ideas

Scratchpad supports Markdown.

---

## Description

Each task contains a description.

Description supports Markdown.

Typical contents:

- Goals
- Requirements
- References
- Links

---

## Markdown

The following fields support Markdown:

- Description
- Scratchpad

Support should include:

- Headings
- Lists
- Checkboxes
- Code blocks
- Inline code
- Links
- Bold
- Italic

---

## Archive

Completed tasks are searchable.

The archive is read-only.

Future versions may allow restoring archived tasks.

---

## Search

Search should perform full-text matching across:

- Title
- Description
- Scratchpad

Search should include archived tasks.

---

## Filtering

The user can filter the active list by project.

Filtering does not change task ordering.

---

# Persistence

All changes are saved immediately.

No explicit Save command exists.

Application startup restores:

- Active task list
- Archive
- Window state
- Last selected project filter

---

# Keyboard Shortcuts

Minimum shortcuts:

| Shortcut | Action |
|-----------|--------|
| Ctrl+N | New task |
| Enter | Edit selected task |
| Ctrl+Up | Move task up |
| Ctrl+Down | Move task down |
| Delete | Complete selected task |
| Ctrl+F | Search |
| Escape | Cancel current edit |

---

# Non-functional Requirements

## Performance

The application should feel instantaneous.

Typical workload:

- 100–500 active tasks
- Several thousand archived tasks

All common operations should complete in well under 100 ms.

---

## Simplicity

Avoid features such as:

- Due dates
- Estimates
- Priorities
- Status values
- Kanban boards
- Notifications
- Multiple users
- Comments
- Attachments
- Dependencies
- Time tracking

---

## Technology

Frontend:

- Rust
- Web

Backend:

- Local storage only

No server required.

---

# Future Ideas (Not MVP)

- Task dependencies
- Task restore from archive
- Multiple ordered stacks
- Tags
- Favorites
- Rich Markdown preview
- Drag-and-drop between projects
- GitHub issue integration
- Daily work journal
- AI-assisted task summarization
- AI-assisted scratchpad cleanup
- Task history
- Export / Import
```
