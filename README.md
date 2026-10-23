# Dynexus Indexer

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
| `GET /health`                                   | Liveness + latest indexed version                           |
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

## Development

```bash
cargo fmt --all
cargo xclippy           # clippy with -D warnings
cargo test --workspace  # DB tests run when DATABASE_URL is set (any Postgres)
```
