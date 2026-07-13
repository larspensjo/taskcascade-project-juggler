# TaskCascade

TaskCascade is a lightweight, personal coordination tool for developers: one ordered task stack, rapid reprioritisation, and Markdown notes that persist immediately to a local SQLite database.

## Run locally

Prerequisites: a current Rust toolchain and Node.js/npm.

```powershell
./scripts/start.ps1
```

The development UI opens at `http://127.0.0.1:5173`; the local API runs at port 8080. Development data goes to `.local/data`. For a standalone backend, set `TASKCASCADE_DATA_DIR` to choose the SQLite directory; otherwise it uses `Documents/TaskCascade`.

## MVP features

- One globally ordered active task list, with drag-and-drop and keyboard reordering
- Projects, filters, instant task editing, and Markdown description/scratchpad previews
- Drag tasks onto Archive in the sidebar to complete them into a read-only searchable archive
- Drag tasks onto Deleted to trash them and drag them back out to restore; deleted tasks are kept out of search but never destroyed
- Search across task titles, descriptions, scratchpads, active work, and archive
- Immediate local persistence and restoration of the project filter

See [the requirements](docs/RequirementSpecification.md), [architecture](docs/Design.HighLevel.md), and [decision log](docs/DecisionLog.md).
