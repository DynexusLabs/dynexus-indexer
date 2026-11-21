import { dispatchBody, type RunSummary, shouldDispatch } from "./logic";

interface Env {
  /** Fine-grained token: repository dynexus-indexer only, Actions read and write. */
  GITHUB_TOKEN: string;
  REPO: string;
  WORKFLOW: string;
  MINUTES: string;
}

const api = (env: Env, path: string) => `https://api.github.com/repos/${env.REPO}${path}`;

const headers = (env: Env) => ({
  Authorization: `Bearer ${env.GITHUB_TOKEN}`,
  Accept: "application/vnd.github+json",
  "X-GitHub-Api-Version": "2022-11-28",
  "User-Agent": "dynexus-index-trigger",
});

async function activeRuns(env: Env): Promise<RunSummary[]> {
  const runs: RunSummary[] = [];
  for (const status of ["in_progress", "queued", "waiting"]) {
    const res = await fetch(
      api(env, `/actions/workflows/${env.WORKFLOW}/runs?status=${status}&per_page=5`),
      { headers: headers(env) },
    );
    if (!res.ok) throw new Error(`list ${status} runs: HTTP ${res.status}`);
    const body = (await res.json()) as { workflow_runs: RunSummary[] };
    runs.push(...body.workflow_runs.map((r) => ({ status })));
  }
  return runs;
}

async function trigger(env: Env): Promise<string> {
  if (!(await activeRuns(env).then(shouldDispatch))) return "skipped: a run is executing and one is queued";
  const res = await fetch(api(env, `/actions/workflows/${env.WORKFLOW}/dispatches`), {
    method: "POST",
    headers: { ...headers(env), "Content-Type": "application/json" },
    body: dispatchBody(Number(env.MINUTES)),
  });
  if (res.status !== 204) throw new Error(`dispatch: HTTP ${res.status} ${await res.text()}`);
  return "dispatched";
}

export default {
  async scheduled(_event: ScheduledController, env: Env): Promise<void> {
    try {
      console.log(await trigger(env));
    } catch (e) {
      // Never log the token; the error text only carries status codes and API messages.
      console.error(e instanceof Error ? e.message : String(e));
      throw e; // surfaces as a failed cron invocation in the Cloudflare dashboard
    }
  },
} satisfies ExportedHandler<Env>;
