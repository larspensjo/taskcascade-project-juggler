export function createTransientMessage(deps: {
  show: (message: string | null) => void;
  durationMs: number;
  timers?: { set: typeof setTimeout; clear: typeof clearTimeout };
}): { post(message: string): void; dispose(): void } {
  const { set, clear } = deps.timers ?? {
    set: setTimeout,
    clear: clearTimeout,
  };
  let timer: ReturnType<typeof setTimeout> | undefined;

  function dispose() {
    if (timer !== undefined) clear(timer);
    timer = undefined;
  }

  return {
    post(message) {
      dispose();
      deps.show(message);
      timer = set(() => {
        timer = undefined;
        deps.show(null);
      }, deps.durationMs);
    },
    dispose,
  };
}
