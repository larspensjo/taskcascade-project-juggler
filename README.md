# TaskCascade

TaskCascade is a lightweight, personal desktop coordination tool for developers: one ordered task stack, rapid reprioritisation, and Markdown notes saved automatically to a local SQLite database.

## Install and run

Prerequisites: Windows, PowerShell 7, a current Rust toolchain, Node.js/npm, and the WebView2 runtime (included with Windows 11).

```powershell
.\scripts\install.ps1
```

Start **TaskCascade** from the Start menu. The installer puts a self-contained release executable in `%LOCALAPPDATA%\Programs\TaskCascade`; rebuilding in the repository does not change it. Close the installed app and re-run the script to update it. To uninstall, close TaskCascade and delete that installation directory and the Start-menu shortcut. For a complete cleanup, also delete `%LOCALAPPDATA%\se.pensjo.taskcascade` (WebView2 profile) and `%APPDATA%\se.pensjo.taskcascade` (window state); development windows use the same folder names with `.dev` appended. Task data in `%LOCALAPPDATA%\TaskCascade` stays unless deliberately removed.

## Where data lives

Your tasks live in `%LOCALAPPDATA%\TaskCascade\taskcascade.sqlite`. Back it up by copying the file while TaskCascade is closed. The installed app refuses a missing database instead of creating one implicitly. For a deliberately empty database, run `.\scripts\install.ps1 -InitDatabase`; it initializes and closes the app once, and leaves an existing database alone.

Runtime diagnostics are appended best-effort to `<data directory>\logs\taskcascade-desktop.log`, including the resolved database path, identity and close outcome.

For older checkouts that still hold tasks in `.local\data`, close all TaskCascade copies and use [scripts/migrate-data.ps1](scripts/migrate-data.ps1) to move and verify them before installing.

## Development

```powershell
.\scripts\start.ps1
# Or use the release profile on the same scratch data:
.\scripts\start.ps1 -Release
```

The launcher builds the frontend and desktop executable, then opens **TaskCascade (dev)** on scratch data in `.local\dev-data`. It can run beside the installed app. Close the window to stop it; Ctrl+C is an abrupt stop and can lose a pending edit. No browser is used to run TaskCascade, and it opens no network port.

There is no live reload. In a debug window, rebuild with `npm run build` from `frontend/`, then press F5. Release builds embed the interface, so rebuild and reopen them to see changes. `-SkipInstall` skips dependency installation; `-SkipBuild` skips both builds.

Run Cargo commands from the repository root: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo fmt --all -- --check`. From `frontend/`, run `npm test`, `npm run check`, and `npm run fmt`.

## MVP features

- One globally ordered active task list, with drag-and-drop and keyboard reordering
- Runs as a desktop window; links open in your system browser; edits are confirmed saved before completing, deleting or reordering active tasks (by drag or keyboard), or closing
- Projects, filters, instant task editing, and Markdown description/scratchpad previews
- Drag tasks onto Archive in the sidebar to complete them into a read-only searchable archive
- Drag tasks onto Deleted to trash them and drag them back out to restore; deleted tasks are kept out of search but never destroyed
- Search across task titles, descriptions, scratchpads, active work, and archive
- Automatic local persistence and restoration of the project filter and window placement

See [the requirements](docs/RequirementSpecification.md), [architecture](docs/Design.HighLevel.md), and [decision log](docs/DecisionLog.md).
