export type Project = { id: string; name: string; createdAt: string };
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
};
export type Bootstrap = {
  projects: Project[];
  activeTasks: Task[];
  archivedTasks: Task[];
  preferences: { key: string; value: string }[];
};

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  const response = await fetch(`/api${path}`, {
    headers: { "Content-Type": "application/json" },
    ...init,
  });
  if (!response.ok) {
    const body = (await response.json().catch(() => ({}))) as {
      message?: string;
    };
    throw new Error(body.message ?? "Something went wrong.");
  }
  return response.status === 204
    ? (undefined as T)
    : (response.json() as Promise<T>);
}

export const api = {
  bootstrap: () => request<Bootstrap>("/bootstrap"),
  search: (q: string) => request<Task[]>(`/search?q=${encodeURIComponent(q)}`),
  createProject: (name: string) =>
    request<Project>("/projects", {
      method: "POST",
      body: JSON.stringify({ name }),
    }),
  createTask: (body: {
    title: string;
    projectId: string;
    description?: string;
  }) => request<Task>("/tasks", { method: "POST", body: JSON.stringify(body) }),
  updateTask: (
    id: string,
    body: Pick<Task, "title" | "projectId" | "description" | "scratchpad">,
  ) =>
    request<Task>(`/tasks/${id}`, {
      method: "PUT",
      body: JSON.stringify(body),
    }),
  completeTask: (id: string) =>
    request<Task>(`/tasks/${id}/complete`, { method: "POST" }),
  restoreTask: (id: string) =>
    request<Task>(`/tasks/${id}/restore`, { method: "POST" }),
  reorderTask: (id: string, targetTaskId: string | null, after: boolean) =>
    request<string[]>(`/tasks/${id}/reorder`, {
      method: "POST",
      body: JSON.stringify({ targetTaskId, after }),
    }),
  savePreference: (key: string, value: string) =>
    request<void>(`/preferences/${key}`, {
      method: "PUT",
      body: JSON.stringify({ value }),
    }),
};
