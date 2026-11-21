# Index trigger (Cloudflare Worker)

GitHub's `schedule` trigger is best-effort and has not honoured `*/5` on this repo. This Worker
calls `workflow_dispatch` on `index.yml` every 5 minutes from Cloudflare's cron, which is reliable.
It skips a tick when one run is executing and another is already queued, because the workflow's
concurrency group would drop the extra dispatch anyway.

## One-time setup

1. **GitHub token** (fine-grained personal access token):
   Settings → Developer settings → Fine-grained tokens → Generate.
   - Resource owner: `DynexusLabs`; repository access: **Only select repositories** →
     `dynexus-indexer`
   - Permissions: **Actions: Read and write** (Metadata read is added automatically). Nothing else.
   - Expiration: 90 days. Put a reminder to rotate it.
2. **Cloudflare**: free account, no card needed.
   ```bash
   cd deploy/cloudflare
   bun install
   bunx wrangler login
   bunx wrangler secret put GITHUB_TOKEN     # paste the token at the prompt, it is not echoed
   bunx wrangler deploy
   ```
3. **Check**: Cloudflare dashboard → Workers → `dynexus-index-trigger` → Logs, or wait 5 minutes and
   look for a `workflow_dispatch` run in the repo's Actions tab.

## Operations

- Change the cadence in `wrangler.toml` (`crons`) and the stream time per run (`MINUTES`), then
  `bunx wrangler deploy`.
- Rotate the token: create a new one, `bunx wrangler secret put GITHUB_TOKEN`, revoke the old one.
- Stop it: `bunx wrangler delete`, or remove the cron trigger in the dashboard.
- Test locally: `bun test` (decision logic). `bunx wrangler dev --test-scheduled` runs the real
  handler and needs `GITHUB_TOKEN` in a git-ignored `.dev.vars` file.

## Security

The token can start and cancel workflows in this one repository and nothing else: it cannot read
the Actions secrets (`DATABASE_URL`, `TRANSACTION_STREAM_API_KEY`), push code or reach other repos.
There is deliberately no destructive manual workflow left in the repo (the one-time
"Reset all data" workflow was removed); `Reset user data` only prunes activity history.
