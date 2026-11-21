# Dynexus Indexer

[![CI](https://github.com/DynexusLabs/dynexus-indexer/actions/workflows/ci.yml/badge.svg)](https://github.com/DynexusLabs/dynexus-indexer/actions/workflows/ci.yml) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) [![Network: Aptos testnet](https://img.shields.io/badge/network-Aptos%20testnet-00d9ff.svg)](https://aptos.dev)

Rust indexer and read API for the Dynexus protocol (Forge, Swap, Core) on Aptos.

The web app used to call one view function per token/pool on every page load, and had no
way to show history (swaps, volume, charts, votes) because that only exists in events.
This service streams DYNEXUS events once, stores them in Postgres, and serves lists,
history and aggregates over HTTP.

```
Aptos Transaction Stream (gRPC, server-side filtered)
        │  only txns emitting DYNEXUS events
        ▼
dynexus-processor ──► Postgres ◄── dynexus-api (axum) ◄── dynexus-web
   (Aptos indexer SDK,           (Diesel schema,
    Diesel, exactly-once)         migrations)
```

## Crates

| Crate               | Role                                                                                    |
| ------------------- | --------------------------------------------------------------------------------------- |
| `dynexus-db`        | Diesel schema, models and embedded migrations shared by both binaries                   |
| `dynexus-processor` | Streams transactions via `aptos-indexer-processor-sdk`, decodes events, writes Postgres |
| `dynexus-api`       | axum + diesel-async read API                                                            |

## How it avoids RPC load

- **Server-side filtering** — `transaction_filter` in the processor config makes the
  Transaction Stream send only transactions that emit events from the DYNEXUS address.
- **No follow-up RPC** — token name/symbol/decimals are read from the
  `0x1::fungible_asset::Metadata` write in the same transaction's write set.
- **Checkpointing** — the SDK stores the last processed version, so restarts resume
  instead of re-reading history.
- **One query instead of N** — the web app gets every pool with symbols, reserves and 24h
  volume from `GET /pools` instead of a view call per pool.

## Correctness

Each batch is written in one database transaction. Every event is first inserted into
`dynexus_events` keyed by `(transaction_version, event_index)` with `ON CONFLICT DO NOTHING`,
and typed tables are only updated if that row was new. Replaying a batch after a crash is
a no-op, so delta updates (`lp_supply + minted`, stake totals, bribe sums) stay exact.
On-chain integers are stored as `NUMERIC` and served as decimal strings (u64/u256 safe in JS).

## API

| Endpoint                                        | Description                                                 |
| ----------------------------------------------- | ----------------------------------------------------------- |
| `GET /health`                                   | Liveness, processor checkpoint, last DYNEXUS event version  |
| `GET /stats`                                    | Token/pool counts, 24h swaps & traders, total veDYNA locked |
| `GET /tokens?creator=&q=&limit=&offset=`        | Factory tokens, newest first; `q` searches name/symbol      |
| `GET /tokens/{address}`                         | Token + pools containing it                                 |
| `GET /pools?token=&limit=&offset=`              | Pools with symbols, reserves, LP supply, 24h volume, gauge  |
| `GET /pools/{address}`                          | Single pool                                                 |
| `GET /pools/{address}/swaps`                    | Swap history                                                |
| `GET /pools/{address}/liquidity`                | Add/remove liquidity history                                |
| `GET /pools/{address}/candles?interval=&limit=` | OHLC of `reserve_y / reserve_x`                             |
| `GET /gauges`                                   | Gauges with current-epoch votes and bribes                  |
| `GET /epochs/{epoch}`                           | Emission distribution, per-pool votes, bribes               |
| `GET /users/{address}/activity`                 | All DYNEXUS events for an account                           |
| `GET /users/{address}/governance`               | veDYNA lock, recent votes, gauge stakes                     |

## Run locally

Prerequisites: Rust (stable), Postgres, `libdw-dev` + `pkg-config` (required by the Aptos
SDK), [bun](https://bun.sh) for git hooks, and a Transaction Stream API key from
[geomi.dev](https://geomi.dev).

```bash
bun install                                   # git hooks
cp .env.example .env
cp config/processor.example.yaml config/processor.yaml   # add auth_token + starting_version
set -a; . ./.env; set +a
cargo run -p dynexus-processor -- --config-path config/processor.yaml
cargo run -p dynexus-api
```

### Docker

```bash
cp .env.example .env
cp config/processor.example.yaml config/processor.yaml
# in config/processor.yaml use host "postgres" instead of "localhost"
docker compose up --build
```

## Hosted testnet setup without a server (GitHub Actions + Neon + Render)

The `Index` workflow runs every 5 minutes for 2 minutes from the stored checkpoint, so each
run only has a few minutes of chain to catch up. When GitHub honours the cron, data is at most
~10 minutes behind the head. GitHub treats `schedule` as best-effort, though: on a quiet repo it
has delayed or dropped runs for hours, so the real lag can be much larger. Anything that needs
live numbers (reserves, balances, your own transactions) must come from the chain, as the web app
does; the indexer is for history and aggregates. To keep it fresh, trigger `Index` externally
(for example a cron service calling `workflow_dispatch`). It reuses a cached binary instead of rebuilding. The `Reset user data` workflow
applies the biweekly reset to Neon. This needs a **public** repository: it uses ~860 runner
minutes a day, and private repos get 2,000 a month. Secrets (`DATABASE_URL`,
`TRANSACTION_STREAM_API_KEY`) stay in GitHub Actions secrets. After redeploying the contracts
set `DYNEXUS_ADDRESS` and `STARTING_VERSION` in `index.yml`. To clear a backlog, run `Index`
manually with `minutes` up to 340.

## Hosted testnet setup (Oracle Cloud Always Free)

One always-on VM runs the whole stack from [`deploy/oracle`](deploy/oracle): the processor
streams continuously and stays within seconds of the chain head, the API sits behind Caddy
(automatic HTTPS), and Postgres is reachable only inside the compose network.

Why not a scheduled job: the Transaction Stream walks every version even with the server-side
filter (~13k versions/s). Testnet grows ~16M versions/day, so the processor needs ~20 min of
streaming a day just to keep pace. The earlier GitHub Actions schedule gave it 16 min and fell
further behind every day. If you run this VM, disable the `Index` workflow (it is the no-server
alternative described above) so two processors do not write the same database.

1. **VM**: in OCI create an instance with shape `VM.Standard.A1.Flex` (Ampere, Always Free
   eligible; 2 OCPU / 12 GB is plenty) and image Ubuntu 24.04. Note the public IP.
2. **Open ports 80 and 443**: in the subnet's security list add ingress rules for TCP 80 and
   443 from `0.0.0.0/0`. Oracle's Ubuntu image also rejects them in iptables:

   ```bash
   sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 80 -j ACCEPT
   sudo iptables -I INPUT 6 -m state --state NEW -p tcp --dport 443 -j ACCEPT
   sudo netfilter-persistent save
   ```

3. **Docker**:

   ```bash
   curl -fsSL https://get.docker.com | sudo sh
   sudo usermod -aG docker "$USER"   # log out and back in
   ```

4. **Configure**:

   ```bash
   git clone https://github.com/DynexusLabs/dynexus-indexer.git && cd dynexus-indexer/deploy/oracle
   cp .env.example .env && chmod 600 .env
   # DOMAIN: your domain (A record -> VM IP) or <ip-with-dashes>.sslip.io
   # POSTGRES_PASSWORD: openssl rand -hex 24
   # TRANSACTION_STREAM_API_KEY: from geomi.dev
   ```

5. **Start**: `docker compose up -d --build`. The first build compiles the processor on the
   VM (~15–25 min on 2 OCPU). On an empty database the processor starts at
   `STARTING_VERSION` and catches up to the chain head (several hours for testnet's backlog),
   then follows it live. Progress: `curl https://$DOMAIN/health` (`processed_version`).

   To skip most of the catch-up, copy the existing Neon data before the first start:

   ```bash
   docker compose up -d postgres
   # runs pg_dump inside the postgres container (needs Neon on Postgres 16 or older)
   docker compose exec -T postgres sh -c \
     'pg_dump --no-owner --no-acl "$0" | psql -v ON_ERROR_STOP=1 -U dynexus dynexus' "$NEON_DATABASE_URL"
   docker compose up -d --build
   ```

6. **Web**: set `NEXT_PUBLIC_INDEXER_URL=https://$DOMAIN` in Vercel and redeploy (the URL is
   baked into the CSP at build time). The Render service and Neon database can then be
   removed.

Operations, from `deploy/oracle`:

```bash
docker compose logs -f processor     # live stream progress
git pull && docker compose up -d --build   # deploy a new version
docker compose exec postgres pg_dump -U dynexus dynexus | gzip > backup.sql.gz
```

Biweekly reset: `deploy/oracle/reset-data.sh` removes user activity history (swaps, liquidity
events, raw event log, votes and bribes of finished epochs) and keeps tokens, pools, gauges,
epochs and live positions (`ve_locks`, `gauge_stakes`, current-epoch votes). The processor
keeps running and keeps its checkpoint, so there is no replay and no downtime. Candles and
volume history restart from empty. Schedule with [`cron.example`](deploy/oracle/cron.example)
(1st and 15th, 03:00 UTC); `RETENTION_DAYS=14` run daily gives a rolling window instead.

Containers restart on crash and on reboot (`restart: unless-stopped`); the processor resumes
from its checkpoint. Logs are capped at 100 MB per service.

## Development

```bash
cargo fmt --all
cargo xclippy           # clippy with -D warnings
cargo test --workspace  # DB tests run when DATABASE_URL is set (any Postgres)
```
