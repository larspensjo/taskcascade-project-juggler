import {
  Archive,
  Check,
  FileText,
  GripVertical,
  Pencil,
  Plus,
  Search,
  Trash2,
  X,
} from "lucide-react";
import {
  type CSSProperties,
  type DragEvent,
  type FormEvent,
  type RefObject,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { type Bootstrap, type Project, type Task, api } from "./api";

type View = "active" | "archive" | "search" | "deleted";
type TaskStatus = "active" | "archived" | "deleted";
type DropTarget = "stack" | "archive" | "trash";
type EditorTab = "description" | "scratchpad";
// Mirrors the backend's default palette so the dialog swatches match
// what auto-assignment produces.
const PALETTE = [
  "#e8833a",
  "#9a6bff",
  "#2bb8a3",
  "#e05c78",
  "#5aa9e6",
  "#a8b545",
  "#d9a03c",
  "#c65bc9",
];
const HEX_COLOR = /^#[0-9a-fA-F]{6}$/;

function statusOf(task: Task): TaskStatus {
  return task.deletedAt ? "deleted" : task.completedAt ? "archived" : "active";
}

function projectStyle(color: string | undefined): CSSProperties {
  return { "--project-color": color } as CSSProperties;
}

// The saved "projectFilter" preference holds a JSON array of project ids.
// Older versions stored "all" or a single project id; treat anything
// unrecognized as "every project".
function parseProjectFilter(
  saved: string | undefined,
  projects: Project[],
): string[] {
  const allIds = projects.map((project) => project.id);
  if (saved?.startsWith("[")) {
    try {
      const parsed: unknown = JSON.parse(saved);
      if (Array.isArray(parsed))
        return allIds.filter((id) => parsed.includes(id));
    } catch {}
    return allIds;
  }
  return saved && allIds.includes(saved) ? [saved] : allIds;
}

function toggleId(selection: string[], id: string): string[] {
  return selection.includes(id)
    ? selection.filter((item) => item !== id)
    : [...selection, id];
}

export function App() {
  const [data, setData] = useState<Bootstrap | null>(null);
  const [view, setView] = useState<View>("active");
  const [selectedProjects, setSelectedProjects] = useState<string[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<Task[]>([]);
  const [newTaskOpen, setNewTaskOpen] = useState(false);
  const [projectDialog, setProjectDialog] = useState<{
    project?: Project;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [dragged, setDragged] = useState<Task | null>(null);
  const [dropHover, setDropHover] = useState<DropTarget | null>(null);
  const searchRef = useRef<HTMLInputElement>(null);
  const titleRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    api
      .bootstrap()
      .then((boot) => {
        setData(boot);
        const savedFilter = boot.preferences.find(
          (preference) => preference.key === "projectFilter",
        )?.value;
        setSelectedProjects(parseProjectFilter(savedFilter, boot.projects));
        setSelectedId(boot.activeTasks[0]?.id ?? null);
      })
      .catch(showError);
  }, []);

  useEffect(() => {
    if (!query.trim()) {
      setResults([]);
      return;
    }
    const timer = window.setTimeout(
      () => api.search(query).then(setResults).catch(showError),
      180,
    );
    return () => window.clearTimeout(timer);
  }, [query]);

  const showError = (reason: unknown) =>
    setError(
      reason instanceof Error ? reason.message : "Something went wrong.",
    );
  const active = data?.activeTasks ?? [];
  const archived = data?.archivedTasks ?? [];
  const deleted = data?.deletedTasks ?? [];
  const visibleTasks = useMemo(() => {
    const source =
      view === "archive"
        ? archived
        : view === "deleted"
          ? deleted
          : view === "search"
            ? results
            : active;
    return source.filter((task) => selectedProjects.includes(task.projectId));
  }, [active, archived, deleted, results, selectedProjects, view]);
  const selected =
    [...active, ...archived, ...deleted].find(
      (task) => task.id === selectedId,
    ) ??
    visibleTasks[0] ??
    null;

  useEffect(() => {
    if (selected && !visibleTasks.some((task) => task.id === selected.id))
      setSelectedId(visibleTasks[0]?.id ?? null);
  }, [selected, visibleTasks]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.ctrlKey && event.key.toLowerCase() === "n") {
        event.preventDefault();
        setNewTaskOpen(true);
      }
      if (event.ctrlKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        searchRef.current?.focus();
      }
      if (event.key === "Escape") {
        setNewTaskOpen(false);
        setProjectDialog(null);
        setQuery("");
        if (query) setView("active");
      }
      if (
        event.key === "Enter" &&
        !isEditingTarget(event.target) &&
        selected &&
        view !== "archive" &&
        view !== "deleted"
      ) {
        event.preventDefault();
        titleRef.current?.focus();
      }
      if (
        event.key === "Delete" &&
        !isEditingTarget(event.target) &&
        selected &&
        view === "active"
      ) {
        event.preventDefault();
        complete(selected);
      }
      if (
        event.ctrlKey &&
        (event.key === "ArrowUp" || event.key === "ArrowDown") &&
        selected &&
        view === "active"
      ) {
        event.preventDefault();
        const index = visibleTasks.findIndex((task) => task.id === selected.id);
        const target = visibleTasks[index + (event.key === "ArrowUp" ? -1 : 1)];
        if (target) reorder(selected, target, event.key === "ArrowDown");
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [query, selected, view, visibleTasks]);

  async function changeSelection(next: string[]) {
    setSelectedProjects(next);
    try {
      await api.savePreference("projectFilter", JSON.stringify(next));
    } catch (reason) {
      showError(reason);
    }
  }
  async function saveProject(
    input: { name: string; color: string | null },
    existing?: Project,
  ) {
    try {
      if (existing) {
        const project = await api.updateProject(existing.id, {
          name: input.name,
          color: input.color ?? existing.color,
        });
        setData(
          (current) =>
            current && {
              ...current,
              projects: current.projects
                .map((item) => (item.id === project.id ? project : item))
                .sort((a, b) => a.name.localeCompare(b.name)),
            },
        );
      } else {
        const project = await api.createProject(
          input.name,
          input.color ?? undefined,
        );
        setData(
          (current) =>
            current && {
              ...current,
              projects: [...current.projects, project].sort((a, b) =>
                a.name.localeCompare(b.name),
              ),
            },
        );
        changeSelection([project.id]);
      }
      setProjectDialog(null);
    } catch (reason) {
      showError(reason);
    }
  }
  async function createTask(input: {
    title: string;
    projectId: string;
    description?: string;
  }) {
    try {
      const task = await api.createTask(input);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: [task, ...current.activeTasks],
          },
      );
      setView("active");
      setSelectedId(task.id);
      setNewTaskOpen(false);
    } catch (reason) {
      showError(reason);
    }
  }
  async function updateTask(updated: Task) {
    try {
      const task = await api.updateTask(updated.id, updated);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: current.activeTasks.map((item) =>
              item.id === task.id ? task : item,
            ),
          },
      );
    } catch (reason) {
      showError(reason);
    }
  }
  async function complete(task: Task) {
    try {
      const completed = await api.completeTask(task.id);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: current.activeTasks.filter(
              (item) => item.id !== task.id,
            ),
            archivedTasks: [completed, ...current.archivedTasks],
          },
      );
      setSelectedId(null);
    } catch (reason) {
      showError(reason);
    }
  }
  async function restore(task: Task) {
    try {
      const restored = await api.restoreTask(task.id);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: [restored, ...current.activeTasks],
            archivedTasks: current.archivedTasks.filter(
              (item) => item.id !== task.id,
            ),
          },
      );
      setView("active");
      setSelectedId(restored.id);
    } catch (reason) {
      showError(reason);
    }
  }
  async function moveToTrash(task: Task) {
    try {
      const trashed = await api.deleteTask(task.id);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: current.activeTasks.filter(
              (item) => item.id !== task.id,
            ),
            archivedTasks: current.archivedTasks.filter(
              (item) => item.id !== task.id,
            ),
            deletedTasks: [trashed, ...current.deletedTasks],
          },
      );
      setSelectedId(null);
    } catch (reason) {
      showError(reason);
    }
  }
  async function undelete(task: Task, to: "stack" | "archive") {
    try {
      const undeleted = await api.undeleteTask(task.id, to);
      setData(
        (current) =>
          current && {
            ...current,
            deletedTasks: current.deletedTasks.filter(
              (item) => item.id !== task.id,
            ),
            activeTasks:
              to === "stack"
                ? [undeleted, ...current.activeTasks]
                : current.activeTasks,
            archivedTasks:
              to === "archive"
                ? [undeleted, ...current.archivedTasks]
                : current.archivedTasks,
          },
      );
    } catch (reason) {
      showError(reason);
    }
  }
  function dropAction(target: DropTarget, task: Task): (() => void) | null {
    const status = statusOf(task);
    if (target === "archive" && status === "active")
      return () => complete(task);
    if (target === "archive" && status === "deleted")
      return () => undelete(task, "archive");
    if (target === "trash" && status !== "deleted")
      return () => moveToTrash(task);
    if (target === "stack" && status === "archived") return () => restore(task);
    if (target === "stack" && status === "deleted")
      return () => undelete(task, "stack");
    return null;
  }
  function dropTargetProps(target: DropTarget) {
    return {
      onDragOver: (event: DragEvent<HTMLButtonElement>) => {
        if (dragged && dropAction(target, dragged)) {
          event.preventDefault();
          setDropHover(target);
        }
      },
      onDragLeave: () =>
        setDropHover((current) => (current === target ? null : current)),
      onDrop: (event: DragEvent<HTMLButtonElement>) => {
        event.preventDefault();
        if (dragged) dropAction(target, dragged)?.();
        setDragged(null);
        setDropHover(null);
      },
    };
  }
  const endDrag = () => {
    setDragged(null);
    setDropHover(null);
  };
  async function reorder(task: Task, target: Task, after: boolean) {
    try {
      const order = await api.reorderTask(task.id, target.id, after);
      setData(
        (current) =>
          current && {
            ...current,
            activeTasks: order.flatMap((id) => {
              const task = current.activeTasks.find((item) => item.id === id);
              return task ? [task] : [];
            }),
          },
      );
    } catch (reason) {
      showError(reason);
    }
  }
  function runSearch(value: string) {
    setQuery(value);
    setView(value.trim() ? "search" : "active");
  }

  if (!data) return <main className="loading">Opening your task stack…</main>;
  return (
    <main className="app-shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-mark">↗</span>
          <span>TaskCascade</span>
        </div>
        <label className="search">
          <Search size={16} />
          <input
            ref={searchRef}
            value={query}
            onChange={(event) => runSearch(event.target.value)}
            placeholder="Search tasks…"
          />
          <kbd>Ctrl F</kbd>
        </label>
        <button
          type="button"
          className="primary"
          onClick={() => setNewTaskOpen(true)}
        >
          <Plus size={17} />
          New task <kbd>Ctrl N</kbd>
        </button>
      </header>
      <div className="workspace">
        <aside className="sidebar">
          <div className="sidebar-heading">WORKSPACE</div>
          <button
            type="button"
            className={
              dropHover === "stack" ? "nav-item drop-ready" : "nav-item"
            }
            onClick={() => {
              setView("active");
              changeSelection(data.projects.map((project) => project.id));
            }}
            {...dropTargetProps("stack")}
          >
            <FileText size={16} />
            All tasks <span>{active.length}</span>
          </button>
          <div className="sidebar-heading with-action">
            PROJECTS{" "}
            <button
              type="button"
              aria-label="Add project"
              onClick={() => setProjectDialog({})}
            >
              <Plus size={15} />
            </button>
          </div>
          {data.projects.map((project) => (
            <div
              key={project.id}
              className="project-row"
              style={projectStyle(project.color)}
            >
              <button
                type="button"
                className={
                  view === "active" && selectedProjects.includes(project.id)
                    ? "nav-item active"
                    : "nav-item"
                }
                onClick={(event) => {
                  setView("active");
                  changeSelection(
                    event.ctrlKey || event.metaKey
                      ? toggleId(selectedProjects, project.id)
                      : [project.id],
                  );
                }}
              >
                <span className="project-dot" />
                {project.name}
                <span>
                  {
                    active.filter((task) => task.projectId === project.id)
                      .length
                  }
                </span>
              </button>
              <button
                type="button"
                className="edit-project"
                aria-label={`Edit ${project.name}`}
                onClick={() => setProjectDialog({ project })}
              >
                <Pencil size={13} />
              </button>
            </div>
          ))}
          <div className="sidebar-spacer" />
          <button
            type="button"
            className={`nav-item${view === "archive" ? " active" : ""}${
              dropHover === "archive" ? " drop-ready" : ""
            }`}
            onClick={() => {
              setView("archive");
              setQuery("");
            }}
            {...dropTargetProps("archive")}
          >
            <Archive size={16} />
            Archive <span>{archived.length}</span>
          </button>
          <button
            type="button"
            className={`nav-item${view === "deleted" ? " active" : ""}${
              dropHover === "trash" ? " drop-ready" : ""
            }`}
            onClick={() => {
              setView("deleted");
              setQuery("");
            }}
            {...dropTargetProps("trash")}
          >
            <Trash2 size={16} />
            Deleted <span>{deleted.length}</span>
          </button>
        </aside>
        <section className="task-pane">
          <div className="pane-heading">
            <div>
              <span className="eyebrow">
                {view === "archive"
                  ? "Completed work"
                  : view === "deleted"
                    ? "Removed work"
                    : view === "search"
                      ? "Search results"
                      : selectedProjects.length === data.projects.length
                        ? "Your ordered stack"
                        : selectedProjects.length === 0
                          ? "No projects selected"
                          : selectedProjects.length === 1
                            ? data.projects.find(
                                (project) => project.id === selectedProjects[0],
                              )?.name
                            : `${selectedProjects.length} projects`}
              </span>
              <h1>
                {view === "archive"
                  ? "Archive"
                  : view === "deleted"
                    ? "Deleted"
                    : view === "search"
                      ? `Results for “${query}”`
                      : "What’s next"}
              </h1>
            </div>
            <span className="count">{visibleTasks.length}</span>
          </div>
          <TaskList
            tasks={visibleTasks}
            selectedId={selected?.id ?? null}
            projects={data.projects}
            draggable={view !== "search"}
            canReorder={view === "active"}
            dragged={dragged}
            onSelect={(task) => setSelectedId(task.id)}
            onReorder={reorder}
            onDragStart={setDragged}
            onDragEnd={endDrag}
          />
        </section>
        <section className="detail-pane">
          {selected ? (
            <TaskEditor
              key={selected.id}
              task={selected}
              projects={data.projects}
              status={statusOf(selected)}
              onUpdate={updateTask}
              titleRef={titleRef}
            />
          ) : (
            <div className="empty-detail">
              <Check size={28} />
              <h2>Nothing selected</h2>
              <p>Create a task or choose one from the stack.</p>
            </div>
          )}
        </section>
      </div>
      {error && (
        <div className="toast" role="alert">
          {error}
          <button type="button" onClick={() => setError(null)}>
            <X size={16} />
          </button>
        </div>
      )}
      {newTaskOpen && (
        <NewTaskDialog
          projects={data.projects}
          initialProject={
            data.projects.find((project) =>
              selectedProjects.includes(project.id),
            )?.id ?? data.projects[0]?.id
          }
          onCreate={createTask}
          onClose={() => setNewTaskOpen(false)}
        />
      )}
      {projectDialog && (
        <ProjectDialog
          project={projectDialog.project}
          onSave={(input) => saveProject(input, projectDialog.project)}
          onClose={() => setProjectDialog(null)}
        />
      )}
    </main>
  );
}

function TaskList({
  tasks,
  selectedId,
  projects,
  draggable,
  canReorder,
  dragged,
  onSelect,
  onReorder,
  onDragStart,
  onDragEnd,
}: {
  tasks: Task[];
  selectedId: string | null;
  projects: Project[];
  draggable: boolean;
  canReorder: boolean;
  dragged: Task | null;
  onSelect: (task: Task) => void;
  onReorder: (task: Task, target: Task, after: boolean) => void;
  onDragStart: (task: Task) => void;
  onDragEnd: () => void;
}) {
  if (!tasks.length)
    return (
      <div className="empty-list">
        <FileText size={24} />
        <h2>No tasks here</h2>
        <p>Capture the next useful piece of work.</p>
      </div>
    );
  const drop = (event: DragEvent, target: Task) => {
    event.preventDefault();
    const bounds = event.currentTarget.getBoundingClientRect();
    const after = event.clientY > bounds.top + bounds.height / 2;
    if (dragged && dragged.id !== target.id) onReorder(dragged, target, after);
    onDragEnd();
  };
  return (
    <ol className="task-list">
      {tasks.map((task, index) => {
        const project = projects.find(
          (candidate) => candidate.id === task.projectId,
        );
        return (
          <li
            key={task.id}
            className={
              selectedId === task.id ? "task-row selected" : "task-row"
            }
            style={projectStyle(project?.color)}
            draggable={draggable}
            onDragStart={() => onDragStart(task)}
            onDragEnd={onDragEnd}
            onDragOver={(event) => {
              if (canReorder) event.preventDefault();
            }}
            onDrop={(event) => {
              if (canReorder) drop(event, task);
            }}
          >
            <button
              type="button"
              className="task-select"
              onClick={() => onSelect(task)}
            >
              <span className="task-order">{index + 1}</span>
              {draggable && <GripVertical className="grab" size={17} />}
              <span className="task-copy">
                <strong>{task.title}</strong>
                <span>{project?.name ?? "Unknown project"}</span>
              </span>
            </button>
          </li>
        );
      })}
    </ol>
  );
}

function TaskEditor({
  task,
  projects,
  status,
  onUpdate,
  titleRef,
}: {
  task: Task;
  projects: Project[];
  status: TaskStatus;
  onUpdate: (task: Task) => void;
  titleRef: RefObject<HTMLInputElement | null>;
}) {
  const [draft, setDraft] = useState(task);
  const [tab, setTab] = useState<EditorTab>("description");
  const [preview, setPreview] = useState(false);
  const saveTimer = useRef<number | undefined>(undefined);
  function change(patch: Partial<Task>) {
    const next = { ...draft, ...patch };
    setDraft(next);
    window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(() => onUpdate(next), 450);
  }
  useEffect(() => () => window.clearTimeout(saveTimer.current), []);
  const content = tab === "description" ? draft.description : draft.scratchpad;
  return (
    <div className="editor">
      <div className="editor-meta">
        <span>
          {status === "deleted"
            ? `Deleted ${formatDate(task.deletedAt ?? task.modifiedAt)}`
            : status === "archived"
              ? `Completed ${formatDate(task.completedAt ?? task.modifiedAt)}`
              : `Updated ${formatDate(task.modifiedAt)}`}
        </span>
      </div>
      {status !== "active" ? (
        <>
          <h1>{task.title}</h1>
          <div className="tag">
            {projects.find((project) => project.id === task.projectId)?.name}
          </div>
          <ReadOnlySection title="Description" value={task.description} />
          <ReadOnlySection title="Scratchpad" value={task.scratchpad} />
        </>
      ) : (
        <>
          <input
            ref={titleRef}
            className="title-input"
            value={draft.title}
            onChange={(event) => change({ title: event.target.value })}
            aria-label="Task title"
          />
          <label className="project-select">
            Project
            <select
              value={draft.projectId}
              onChange={(event) => change({ projectId: event.target.value })}
            >
              {projects.map((project) => (
                <option key={project.id} value={project.id}>
                  {project.name}
                </option>
              ))}
            </select>
          </label>
          <div className="editor-tabs">
            <button
              type="button"
              className={tab === "description" ? "active" : ""}
              onClick={() => setTab("description")}
            >
              Description
            </button>
            <button
              type="button"
              className={tab === "scratchpad" ? "active" : ""}
              onClick={() => setTab("scratchpad")}
            >
              Scratchpad
            </button>
            <span />
            <button
              type="button"
              className="preview-toggle"
              onClick={() => setPreview(!preview)}
            >
              {preview ? "Write" : "Preview"}
            </button>
          </div>
          {preview ? (
            <Markdown value={content} empty={`No ${tab} yet.`} />
          ) : (
            <textarea
              value={content}
              onChange={(event) =>
                change(
                  tab === "description"
                    ? { description: event.target.value }
                    : { scratchpad: event.target.value },
                )
              }
              placeholder={
                tab === "description"
                  ? "Goals, requirements, links…"
                  : "Notes, investigation, snippets, future ideas…"
              }
            />
          )}
          <p className="autosave">Saved automatically · Markdown supported</p>
        </>
      )}
    </div>
  );
}

function ReadOnlySection({ title, value }: { title: string; value: string }) {
  return (
    <section className="readonly-section">
      <h2>{title}</h2>
      <Markdown value={value} empty={`No ${title.toLowerCase()} was saved.`} />
    </section>
  );
}
function Markdown({ value, empty }: { value: string; empty: string }) {
  return (
    <div className={value.trim() ? "markdown" : "markdown empty-markdown"}>
      {value.trim() ? (
        <ReactMarkdown remarkPlugins={[remarkGfm]}>{value}</ReactMarkdown>
      ) : (
        empty
      )}
    </div>
  );
}
function NewTaskDialog({
  projects,
  initialProject,
  onCreate,
  onClose,
}: {
  projects: Project[];
  initialProject?: string;
  onCreate: (input: {
    title: string;
    projectId: string;
    description?: string;
  }) => void;
  onClose: () => void;
}) {
  const [title, setTitle] = useState("");
  const [projectId, setProjectId] = useState(initialProject ?? "");
  const [description, setDescription] = useState("");
  function submit(event: FormEvent) {
    event.preventDefault();
    if (title.trim() && projectId) onCreate({ title, projectId, description });
  }
  return (
    <Dialog title="New task" onClose={onClose}>
      <form onSubmit={submit}>
        <label>
          Title
          <input
            value={title}
            onChange={(event) => setTitle(event.target.value)}
            placeholder="What needs your attention?"
          />
        </label>
        <label>
          Project
          <select
            value={projectId}
            onChange={(event) => setProjectId(event.target.value)}
          >
            {projects.map((project) => (
              <option key={project.id} value={project.id}>
                {project.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Description <span className="optional">optional</span>
          <textarea
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            placeholder="A little context helps future you."
          />
        </label>
        <div className="dialog-actions">
          <button type="button" className="secondary" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={!title.trim() || !projectId}
          >
            Create task
          </button>
        </div>
      </form>
    </Dialog>
  );
}
function ProjectDialog({
  project,
  onSave,
  onClose,
}: {
  project?: Project;
  onSave: (input: { name: string; color: string | null }) => void;
  onClose: () => void;
}) {
  const [name, setName] = useState(project?.name ?? "");
  // Empty string means "let the backend pick a free palette color".
  const [color, setColor] = useState(project?.color ?? "");
  const colorValid = color === "" ? !project : HEX_COLOR.test(color);
  return (
    <Dialog title={project ? "Edit project" : "New project"} onClose={onClose}>
      <form
        onSubmit={(event) => {
          event.preventDefault();
          if (name.trim() && colorValid)
            onSave({ name, color: color === "" ? null : color });
        }}
      >
        <label>
          Name
          <input
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="e.g. Engine"
          />
        </label>
        <label>
          Color {!project && <span className="optional">optional</span>}
          <span className="color-field">
            {PALETTE.map((swatch) => (
              <button
                type="button"
                key={swatch}
                className={
                  color.toLowerCase() === swatch ? "swatch selected" : "swatch"
                }
                style={{ background: swatch }}
                aria-label={`Use ${swatch}`}
                onClick={() => setColor(swatch)}
              />
            ))}
            <input
              value={color}
              onChange={(event) => setColor(event.target.value)}
              placeholder="auto"
              aria-label="Hex color"
            />
          </span>
        </label>
        <div className="dialog-actions">
          <button type="button" className="secondary" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            className="primary"
            disabled={!name.trim() || !colorValid}
          >
            {project ? "Save project" : "Create project"}
          </button>
        </div>
      </form>
    </Dialog>
  );
}
function Dialog({
  title,
  children,
  onClose,
}: { title: string; children: React.ReactNode; onClose: () => void }) {
  return (
    <div className="dialog-backdrop" onMouseDown={onClose}>
      <dialog
        open
        className="dialog"
        aria-label={title}
        onMouseDown={(event) => event.stopPropagation()}
      >
        <div className="dialog-header">
          <h2>{title}</h2>
          <button type="button" onClick={onClose} aria-label="Close">
            <X size={18} />
          </button>
        </div>
        {children}
      </dialog>
    </div>
  );
}
function formatDate(value: string) {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  }).format(new Date(value));
}
function isEditingTarget(target: EventTarget | null) {
  return (
    target instanceof HTMLInputElement ||
    target instanceof HTMLTextAreaElement ||
    target instanceof HTMLSelectElement
  );
}
