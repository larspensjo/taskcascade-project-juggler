import type { View } from "./App";
import type { Task } from "./api";

export type SearchState = {
  attempt: number;
  requested: string;
  shownAttempt: number;
  tasks: Task[];
};

export const initialSearch: SearchState = {
  attempt: 0,
  requested: "",
  shownAttempt: 0,
  tasks: [],
};

export function searchStarted(state: SearchState, query: string): SearchState {
  const attempt = state.attempt + 1;
  return query.trim()
    ? { ...state, attempt, requested: query }
    : { attempt, requested: query, shownAttempt: attempt, tasks: [] };
}

export function searchResolved(
  state: SearchState,
  attempt: number,
  tasks: Task[],
): SearchState {
  return attempt === state.attempt
    ? { ...state, shownAttempt: attempt, tasks }
    : state;
}

export function searchSettled(view: View, state: SearchState): boolean {
  return view !== "search" || state.shownAttempt === state.attempt;
}
