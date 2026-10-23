import type { CloseResult } from "./closeFlow";

declare global {
  interface Window {
    taskcascadeBeforeClose?: () => Promise<CloseResult>;
  }
}

export function registerBeforeClose(
  handler: () => Promise<CloseResult>,
): () => void {
  window.taskcascadeBeforeClose = handler;
  return () => {
    if (window.taskcascadeBeforeClose === handler)
      window.taskcascadeBeforeClose = undefined;
  };
}
