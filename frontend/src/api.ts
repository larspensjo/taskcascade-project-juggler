export type Project = {
  id: string;
  name: string;
  color: string;
  createdAt: string;
};
export type Task = {
  id: string;
  title: string;
  description: string;
  scratchpad: string;
  projectId: string;
  position: number;
  createdAt: string;
  modifiedAt: string;
  completedAt: string | null;
  deletedAt: string | null;
};
export type Bootstrap = {
  projects: Project[];
  activeTasks: Task[];
  archivedTasks: Task[];
  deletedTasks: Task[];
  preferences: { key: string; value: string }[];
};

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message);
    this.name = "ApiError";
  }
}

const inFlight = new Set<Promise<unknown>>();

function request<T>(path: string, init?: RequestInit): Promise<T> {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const pending = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  inFlight.add(pending);
  void send<T>(path, init).then(resolve, reject);
  return pending.finally(() => {
    inFlight.delete(pending);
  });
}

async function send<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as {
      message?: string;
    };
    throw new ApiError(
      response.status,
      body.message ?? "Something went wrong.",
    );
  }
  return response.status === 204
    ? (undefined as T)
    : (response.json() as Promise<T>);
}

export async function settle(): Promise<void> {
  while (inFlight.size) await Promise.allSettled([...inFlight]);
}

export function reportClose(
  outcome: "saved" | "failed" | "discarded",
): Promise<void> {
  return request(`/desktop/close-ready?outcome=${outcome}`, { method: "POST" });
}

export const api = {
  bootstrap: () => request<Bootstrap>("/api/bootstrap"),
  search: (q: string) =>
    request<Task[]>(`/api/search?q=${encodeURIComponent(q)}`),
  createProject: (name: string, color?: string) =>
    request<Project>("/api/projects", {
      method: "POST",
      body: JSON.stringify({ name, color }),
    }),
  updateProject: (id: string, body: { name: string; color: string }) =>
    request<Project>(`/api/projects/${id}`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),
  createTask: (body: {
    title: string;
    projectId: string;
    description?: string;
  }) =>
    request<Task>("/api/tasks", { method: "POST", body: JSON.stringify(body) }),
  updateTask: (
    id: string,
    body: Pick<Task, "title" | "projectId" | "description" | "scratchpad">,
  ) =>
    request<Task>(`/api/tasks/${id}`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),
  completeTask: (id: string) =>
    request<Task>(`/api/tasks/${id}/complete`, { method: "POST" }),
  restoreTask: (id: string) =>
    request<Task>(`/api/tasks/${id}/restore`, { method: "POST" }),
  deleteTask: (id: string) =>
    request<Task>(`/api/tasks/${id}/delete`, { method: "POST" }),
  undeleteTask: (id: string, to: "stack" | "archive") =>
    request<Task>(`/api/tasks/${id}/undelete`, {
      method: "POST",
      body: JSON.stringify({ to }),
    }),
  reorderTask: (id: string, targetTaskId: string | null, after: boolean) =>
    request<string[]>(`/api/tasks/${id}/reorder`, {
      method: "POST",
      body: JSON.stringify({ targetTaskId, after }),
    }),
  savePreference: (key: string, value: string) =>
    request<void>(`/api/preferences/${key}`, {
      method: "PUT",
      body: JSON.stringify({ value }),
    }),
};
