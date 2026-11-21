/** Pure decision logic for the Index trigger, kept free of fetch so it can be unit tested. */

export interface RunSummary {
  status: string;
}

/**
 * Dispatch unless the workflow already has one run executing and one waiting: the workflow's
 * concurrency group queues at most one pending run, so more dispatches would be dropped anyway.
 */
export function shouldDispatch(active: RunSummary[]): boolean {
  const running = active.filter((r) => r.status === "in_progress").length;
  const waiting = active.filter((r) => r.status === "queued" || r.status === "waiting").length;
  return !(running >= 1 && waiting >= 1);
}

export function dispatchBody(minutes: number): string {
  if (!Number.isInteger(minutes) || minutes < 1 || minutes > 340) {
    throw new Error("minutes must be an integer between 1 and 340");
  }
  return JSON.stringify({ ref: "main", inputs: { minutes: String(minutes) } });
}
