import type { SaveCoordinator } from "./saveCoordinator";

export type CloseResult = "saved" | "failed";

export async function runCloseFlow(deps: {
  coordinator: SaveCoordinator;
  settle: () => Promise<void>;
  report: (outcome: "saved" | "failed") => Promise<void>;
  showFailure: (error: Error) => void;
  setClosing: (closing: boolean) => void;
  timeoutMs: number;
}): Promise<CloseResult> {
  deps.setClosing(true);
  deps.coordinator.freeze();
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const persistence = (async () => {
      const outcome = await deps.coordinator.flushAll();
      if (!outcome.ok) throw outcome.error;
      await deps.settle();
    })();
    await Promise.race([
      persistence,
      new Promise<never>((_resolve, reject) => {
        timer = setTimeout(
          () =>
            reject(
              new Error("Saving did not finish before the close timeout."),
            ),
          deps.timeoutMs,
        );
      }),
    ]);
    clearTimeout(timer);
    await deps.report("saved");
    return "saved";
  } catch (reason) {
    clearTimeout(timer);
    const error = reason instanceof Error ? reason : new Error(String(reason));
    // A transport failure must still restore editing and preserve the draft.
    try {
      await deps.report("failed");
    } finally {
      deps.coordinator.unfreeze();
      deps.setClosing(false);
      deps.showFailure(error);
    }
    return "failed";
  }
}
