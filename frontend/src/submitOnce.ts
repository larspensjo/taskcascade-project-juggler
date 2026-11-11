export function createSubmitOnce(): (
  run: () => Promise<boolean>,
) => Promise<boolean> {
  let pending: Promise<boolean> | undefined;
  let succeeded = false;

  return (run) => {
    if (succeeded) return Promise.resolve(true);
    if (pending) return pending;

    let resolveRun!: (value: boolean) => void;
    let rejectRun!: (reason: unknown) => void;
    const runResult = new Promise<boolean>((resolve, reject) => {
      resolveRun = resolve;
      rejectRun = reject;
    });
    const current = runResult
      .then((result) => {
        if (result) succeeded = true;
        return result;
      })
      .finally(() => {
        pending = undefined;
      });
    pending = current;

    try {
      Promise.resolve(run()).then(resolveRun, rejectRun);
    } catch (reason) {
      rejectRun(reason);
    }

    return current;
  };
}
