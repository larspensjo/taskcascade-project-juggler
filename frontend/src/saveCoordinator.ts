import type { Task } from "./api";

export type Fields = Pick<
  Task,
  "title" | "projectId" | "description" | "scratchpad"
>;
export type SaveStatus = "clean" | "pending" | "saving" | "failed";
export type SaveOutcome = { ok: true } | { ok: false; error: Error };

// Keep the mounted editor's spelling when the server only normalised it.
// Stored tasks still supply the values everywhere outside the editor.
export function shownFields(
  draft: Fields | undefined,
  lastEdited: Fields | undefined,
  stored: Fields,
): Fields {
  if (draft) return draft;
  if (
    lastEdited &&
    lastEdited.title.trim() === stored.title.trim() &&
    lastEdited.projectId === stored.projectId &&
    lastEdited.description === stored.description &&
    lastEdited.scratchpad === stored.scratchpad
  ) {
    return lastEdited;
  }
  return stored;
}

export type SaveCoordinator = {
  draft(taskId: string): Fields | undefined;
  status(taskId: string): SaveStatus;
  error(taskId: string): Error | undefined;
  edit(taskId: string, fields: Fields): boolean;
  flush(taskId: string): Promise<SaveOutcome>;
  flushAll(): Promise<SaveOutcome>;
  afterSaved<T>(
    taskId: string,
    action: () => Promise<T>,
  ): Promise<{ ok: true; value: T } | { ok: false; error: Error }>;
  freeze(): void;
  unfreeze(): void;
  frozen(): boolean;
  discard(taskId: string): void;
  discardAll(): void;
  subscribe(listener: () => void): () => void;
  version(): number;
};

type Entry = {
  fields: Fields;
  revision: number;
  status: SaveStatus;
  error?: Error;
  timer?: ReturnType<typeof setTimeout>;
  sending?: Promise<SaveOutcome>;
};

export function createSaveCoordinator(options: {
  send: (taskId: string, fields: Fields) => Promise<Task>;
  onSaved: (task: Task) => void;
  delayMs: number;
  timers?: { set: typeof setTimeout; clear: typeof clearTimeout };
}): SaveCoordinator {
  const entries = new Map<string, Entry>();
  const listeners = new Set<() => void>();
  const timers = options.timers ?? { set: setTimeout, clear: clearTimeout };
  let frozen = false;
  let version = 0;
  function notify() {
    version++;
    for (const listener of listeners) listener();
  }
  function clearTimer(entry: Entry) {
    if (entry.timer !== undefined) timers.clear(entry.timer);
    entry.timer = undefined;
  }
  function start(id: string, entry: Entry): Promise<SaveOutcome> {
    if (entry.sending) return entry.sending;
    clearTimer(entry);
    entry.status = "saving";
    entry.error = undefined;
    // Install the shared promise before invoking send or notifying subscribers.
    let resolve!: (outcome: SaveOutcome) => void;
    const sending = new Promise<SaveOutcome>((done) => {
      resolve = done;
    });
    entry.sending = sending;
    const run = async (): Promise<SaveOutcome> => {
      while (entries.get(id) === entry) {
        const revision = entry.revision;
        const fields = entry.fields;
        try {
          const task = await options.send(id, fields);
          if (entries.get(id) !== entry) return { ok: true };
          options.onSaved(task);
          if (entry.revision === revision) {
            entries.delete(id);
            notify();
            return { ok: true };
          }
          // A newer edit owns the draft. Send just that revision next.
          clearTimer(entry);
        } catch (reason) {
          if (entries.get(id) !== entry) return { ok: true };
          if (entry.revision !== revision) {
            // Only failure of the newest revision can fail this flush.
            clearTimer(entry);
            continue;
          }
          const error =
            reason instanceof Error ? reason : new Error(String(reason));
          if (entries.get(id) === entry) {
            clearTimer(entry);
            entry.error = error;
            entry.status = "failed";
            entry.sending = undefined;
            notify();
          }
          return { ok: false, error };
        }
      }
      return { ok: true };
    };
    void run().then(resolve);
    notify();
    return sending;
  }
  const coordinator: SaveCoordinator = {
    draft: (id) => entries.get(id)?.fields,
    status: (id) => entries.get(id)?.status ?? "clean",
    error: (id) => entries.get(id)?.error,
    edit(id, fields) {
      if (frozen) return false;
      const entry = entries.get(id) ?? {
        fields,
        revision: 0,
        status: "pending",
      };
      clearTimer(entry);
      entry.fields = { ...fields };
      entry.revision++;
      entry.error = undefined;
      entry.status = entry.sending ? "saving" : "pending";
      entries.set(id, entry);
      entry.timer = timers.set(() => {
        void start(id, entry);
      }, options.delayMs);
      notify();
      return true;
    },
    flush(id) {
      const entry = entries.get(id);
      if (!entry) return Promise.resolve({ ok: true });
      clearTimer(entry);
      return start(id, entry);
    },
    async flushAll() {
      try {
        await Promise.all(
          [...entries.keys()].map(async (id) => {
            const outcome = await coordinator.flush(id);
            if (!outcome.ok) throw outcome.error;
          }),
        );
        return { ok: true };
      } catch (error) {
        return {
          ok: false,
          error: error instanceof Error ? error : new Error(String(error)),
        };
      }
    },
    async afterSaved(id, action) {
      const outcome = await coordinator.flush(id);
      if (!outcome.ok) return outcome;
      return { ok: true, value: await action() };
    },
    freeze() {
      frozen = true;
      notify();
    },
    unfreeze() {
      frozen = false;
      notify();
    },
    frozen: () => frozen,
    discard(id) {
      const entry = entries.get(id);
      if (!entry) return;
      clearTimer(entry);
      entries.delete(id);
      notify();
    },
    discardAll() {
      for (const entry of entries.values()) clearTimer(entry);
      entries.clear();
      notify();
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    version: () => version,
  };
  return coordinator;
}
