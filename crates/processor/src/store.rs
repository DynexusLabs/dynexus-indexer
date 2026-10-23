//! Persisting a batch of decoded events with Diesel.
//!
//! A batch is written in one database transaction. Each event is first inserted into
//! `dynexus_events` with `ON CONFLICT DO NOTHING`; typed tables are only touched when that
//! insert created a row. Replaying a batch (e.g. after a crash before the SDK saved its
//! checkpoint) is therefore a no-op, which makes delta updates like `lp_supply + minted` safe.

use crate::events::{DynexusEvent, FaMetadata, Num};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::{prelude::*, upsert::excluded};
use diesel_async::{AsyncConnection, AsyncPgConnection, RunQueryDsl, scoped_futures::ScopedFutureExt};
use dynexus_db::{models::*, schema::*};
use std::str::FromStr;

pub struct EventRow {
    pub version: i64,
    pub index: i64,
    pub module: String,
    pub name: String,
    pub account: Option<String>,
    pub data: serde_json::Value,
    /// Transaction timestamp, unix seconds
    pub ts: i64,
    pub event: DynexusEvent,
    /// For `TokenCreated`: the token's metadata from the same transaction's write set
    pub token_metadata: Option<FaMetadata>,
}

type Conn = AsyncPgConnection;

pub async fn write_batch(conn: &mut Conn, rows: Vec<EventRow>) -> QueryResult<usize> {
    conn.transaction(|conn| {
        async move {
            let mut applied = 0;
            for row in &rows {
                let inserted = diesel::insert_into(dynexus_events::table)
                    .values(NewEvent {
                        transaction_version: row.version,
                        event_index: row.index,
                        module: &row.module,
                        name: &row.name,
                        account: row.account.as_deref(),
                        data: &row.data,
                        ts: timestamp(row.ts)?,
                    })
                    .on_conflict_do_nothing()
                    .execute(conn)
                    .await?;
                if inserted == 1 {
                    apply(conn, row).await?;
                    applied += 1;
                }
            }
            Ok(applied)
        }
        .scope_boxed()
    })
    .await
}

fn conversion_error(msg: String) -> diesel::result::Error {
    diesel::result::Error::SerializationError(msg.into())
}

fn dec(n: &Num) -> QueryResult<BigDecimal> {
    BigDecimal::from_str(&n.0).map_err(|e| conversion_error(format!("{}: {e}", n.0)))
}

fn neg(n: &Num) -> QueryResult<BigDecimal> {
    dec(n).map(|d| -d)
}

fn int(n: &Num) -> QueryResult<i64> {
    n.as_i64().map_err(|e| conversion_error(format!("{}: {e}", n.0)))
}

fn timestamp(secs: i64) -> QueryResult<DateTime<Utc>> {
    DateTime::from_timestamp(secs, 0).ok_or_else(|| conversion_error(format!("bad timestamp {secs}")))
}

async fn apply(conn: &mut Conn, row: &EventRow) -> QueryResult<()> {
    use DynexusEvent as E;
    let (v, i, ts) = (row.version, row.index, timestamp(row.ts)?);
    match &row.event {
        E::TokenCreated(e) => {
            let m = row.token_metadata.as_ref();
            diesel::insert_into(tokens::table)
                .values(Token {
                    address: e.token_address.0.clone(),
                    creator: e.creator.0.clone(),
                    name: m.map(|m| m.name.clone()),
                    symbol: m.map(|m| m.symbol.clone()),
                    decimals: m.map(|m| m.decimals as i16),
                    icon_uri: m.map(|m| m.icon_uri.clone()),
                    project_uri: m.map(|m| m.project_uri.clone()),
                    burned: BigDecimal::from(0),
                    created_at: ts,
                    created_version: v,
                })
                .on_conflict_do_nothing()
                .execute(conn)
                .await?;
        }
        E::TokenBurned(e) => {
            diesel::update(tokens::table.find(&e.token_address.0))
                .set(tokens::burned.eq(tokens::burned + dec(&e.amount)?))
                .execute(conn)
                .await?;
        }
        E::PoolCreated(e) => {
            diesel::insert_into(pools::table)
                .values(Pool {
                    address: e.pool_addr.0.clone(),
                    token_x: e.token_x.0.clone(),
                    token_y: e.token_y.0.clone(),
                    creator: e.creator.0.clone(),
                    reserve_x: dec(&e.reserve_x)?,
                    reserve_y: dec(&e.reserve_y)?,
                    lp_supply: dec(&e.lp_token_supply)?,
                    fee_bps: int(&e.fee_percent)? as i32,
                    platform_fee_bps: int(&e.platform_fee_percent)? as i32,
                    created_at: ts,
                    created_version: v,
                    last_version: v,
                })
                .on_conflict_do_nothing()
                .execute(conn)
                .await?;
        }
        E::Swap(e) => {
            diesel::insert_into(swaps::table)
                .values(Swap {
                    transaction_version: v,
                    event_index: i,
                    pool: e.pool_address.0.clone(),
                    trader: e.trader.0.clone(),
                    token_in: e.token_in.0.clone(),
                    token_out: e.token_out.0.clone(),
                    amount_in: dec(&e.amount_in)?,
                    amount_out: dec(&e.amount_out)?,
                    swap_fee: dec(&e.swap_fee)?,
                    platform_fee: dec(&e.platform_fee)?,
                    reserve_x: dec(&e.new_reserve_x)?,
                    reserve_y: dec(&e.new_reserve_y)?,
                    ts,
                })
                .execute(conn)
                .await?;
            let zero = BigDecimal::from(0);
            update_pool(
                conn,
                &e.pool_address.0,
                &e.new_reserve_x,
                &e.new_reserve_y,
                zero,
                v,
            )
            .await?;
        }
        E::LiquidityAdded(e) => {
            diesel::insert_into(liquidity_events::table)
                .values(LiquidityEvent {
                    transaction_version: v,
                    event_index: i,
                    pool: e.pool_address.0.clone(),
                    provider: e.provider.0.clone(),
                    kind: "add".into(),
                    amount_x: dec(&e.amount_x)?,
                    amount_y: dec(&e.amount_y)?,
                    lp_amount: dec(&e.lp_tokens_minted)?,
                    reserve_x: dec(&e.new_reserve_x)?,
                    reserve_y: dec(&e.new_reserve_y)?,
                    ts,
                })
                .execute(conn)
                .await?;
            let delta = dec(&e.lp_tokens_minted)?;
            update_pool(
                conn,
                &e.pool_address.0,
                &e.new_reserve_x,
                &e.new_reserve_y,
                delta,
                v,
            )
            .await?;
        }
        E::LiquidityRemoved(e) => {
            diesel::insert_into(liquidity_events::table)
                .values(LiquidityEvent {
                    transaction_version: v,
                    event_index: i,
                    pool: e.pool_address.0.clone(),
                    provider: e.provider.0.clone(),
                    kind: "remove".into(),
                    amount_x: dec(&e.amount_x)?,
                    amount_y: dec(&e.amount_y)?,
                    lp_amount: dec(&e.lp_tokens_burned)?,
                    reserve_x: dec(&e.new_reserve_x)?,
                    reserve_y: dec(&e.new_reserve_y)?,
                    ts,
                })
                .execute(conn)
                .await?;
            let delta = neg(&e.lp_tokens_burned)?;
            update_pool(
                conn,
                &e.pool_address.0,
                &e.new_reserve_x,
                &e.new_reserve_y,
                delta,
                v,
            )
            .await?;
        }
        E::FeeConfigUpdated(e) => {
            // Fees are global; every pool uses the new values immediately.
            diesel::update(pools::table)
                .set((
                    pools::fee_bps.eq(int(&e.default_fee_percent)? as i32),
                    pools::platform_fee_bps.eq(int(&e.platform_fee_percent)? as i32),
                ))
                .execute(conn)
                .await?;
        }
        E::LockUpdated(e) => {
            diesel::insert_into(ve_locks::table)
                .values(VeLock {
                    user_address: e.user.0.clone(),
                    amount: dec(&e.amount)?,
                    unlock_time: int(&e.unlock_time)?,
                    updated_version: v,
                })
                .on_conflict(ve_locks::user_address)
                .do_update()
                .set((
                    ve_locks::amount.eq(excluded(ve_locks::amount)),
                    ve_locks::unlock_time.eq(excluded(ve_locks::unlock_time)),
                    ve_locks::updated_version.eq(excluded(ve_locks::updated_version)),
                ))
                .execute(conn)
                .await?;
        }
        E::Withdrawn(e) => {
            diesel::delete(ve_locks::table.find(&e.user.0))
                .execute(conn)
                .await?;
        }
        E::Voted(e) => {
            let epoch = int(&e.epoch)?;
            clear_votes(conn, &e.user.0, epoch).await?;
            let rows = e
                .pools
                .iter()
                .zip(&e.votes)
                .filter(|(_, votes)| votes.0 != "0")
                .map(|(pool, votes)| {
                    Ok(Vote {
                        user_address: e.user.0.clone(),
                        epoch,
                        pool: pool.0.clone(),
                        vote_weight: dec(votes)?,
                        transaction_version: v,
                    })
                })
                .collect::<QueryResult<Vec<_>>>()?;
            diesel::insert_into(votes::table)
                .values(&rows)
                .execute(conn)
                .await?;
        }
        E::VotesReset(e) => {
            clear_votes(conn, &e.user.0, int(&e.epoch)?).await?;
        }
        E::EpochDistributed(e) => {
            diesel::insert_into(epochs::table)
                .values(Epoch {
                    epoch: int(&e.epoch)?,
                    emission: dec(&e.emission)?,
                    total_votes: dec(&e.total_votes)?,
                    distributed_version: v,
                })
                .on_conflict_do_nothing()
                .execute(conn)
                .await?;
        }
        E::GaugeCreated(e) => {
            diesel::insert_into(gauges::table)
                .values(Gauge {
                    pool: e.pool.0.clone(),
                    lp_token: e.lp_token.0.clone(),
                    total_staked: BigDecimal::from(0),
                    reward_rate: BigDecimal::from(0),
                    period_finish: 0,
                    created_version: v,
                })
                .on_conflict_do_nothing()
                .execute(conn)
                .await?;
        }
        E::Staked(e) | E::Unstaked(e) => {
            let delta = if matches!(row.event, E::Staked(_)) {
                dec(&e.amount)?
            } else {
                neg(&e.amount)?
            };
            diesel::insert_into(gauge_stakes::table)
                .values(GaugeStake {
                    pool: e.pool.0.clone(),
                    user_address: e.user.0.clone(),
                    staked: delta,
                })
                .on_conflict((gauge_stakes::pool, gauge_stakes::user_address))
                .do_update()
                .set(gauge_stakes::staked.eq(gauge_stakes::staked + excluded(gauge_stakes::staked)))
                .execute(conn)
                .await?;
            diesel::update(gauges::table.find(&e.pool.0))
                .set(gauges::total_staked.eq(dec(&e.total_staked)?))
                .execute(conn)
                .await?;
        }
        E::RewardNotified(e) => {
            diesel::update(gauges::table.find(&e.pool.0))
                .set((
                    gauges::reward_rate.eq(dec(&e.reward_rate)?),
                    gauges::period_finish.eq(int(&e.period_finish)?),
                ))
                .execute(conn)
                .await?;
        }
        E::BribeAdded(e) => {
            diesel::insert_into(bribes::table)
                .values(Bribe {
                    pool: e.pool.0.clone(),
                    epoch: int(&e.epoch)?,
                    token: e.token.0.clone(),
                    amount: dec(&e.amount)?,
                })
                .on_conflict((bribes::pool, bribes::epoch, bribes::token))
                .do_update()
                .set(bribes::amount.eq(bribes::amount + excluded(bribes::amount)))
                .execute(conn)
                .await?;
        }
        E::Other => {}
    }
    Ok(())
}

/// Reserves are absolute values from the event; LP supply moves by `lp_delta`.
async fn update_pool(
    conn: &mut Conn,
    pool: &str,
    reserve_x: &Num,
    reserve_y: &Num,
    lp_delta: BigDecimal,
    version: i64,
) -> QueryResult<usize> {
    diesel::update(pools::table.find(pool))
        .set((
            pools::reserve_x.eq(dec(reserve_x)?),
            pools::reserve_y.eq(dec(reserve_y)?),
            pools::lp_supply.eq(pools::lp_supply + lp_delta),
            pools::last_version.eq(version),
        ))
        .execute(conn)
        .await
}

async fn clear_votes(conn: &mut Conn, user: &str, epoch: i64) -> QueryResult<usize> {
    diesel::delete(
        votes::table
            .filter(votes::user_address.eq(user))
            .filter(votes::epoch.eq(epoch)),
    )
    .execute(conn)
    .await
}

#[cfg(test)]
mod tests {
    //! Runs against a real Postgres when `DATABASE_URL` is set; skipped otherwise.
    use super::*;
    use crate::events::decode;
    use diesel::Connection;
    use diesel_migrations::MigrationHarness;
    use serde_json::json;

    async fn connect() -> Option<AsyncPgConnection> {
        let url = std::env::var("DATABASE_URL").ok()?;
        let mut sync = diesel::PgConnection::establish(&url).expect("connect");
        sync.run_pending_migrations(dynexus_db::MIGRATIONS)
            .expect("migrate");
        Some(AsyncPgConnection::establish(&url).await.expect("async connect"))
    }

    fn row(version: i64, index: i64, module: &str, name: &str, data: serde_json::Value) -> EventRow {
        EventRow {
            version,
            index,
            module: module.into(),
            name: name.into(),
            account: crate::events::actor(&data),
            event: decode(module, name, &data).unwrap(),
            data,
            ts: 1_700_000_000,
            token_metadata: None,
        }
    }

    fn batch(pool: &str, base: i64) -> Vec<EventRow> {
        vec![
            row(
                base,
                0,
                "dex",
                "PoolCratedEvent",
                json!({
                    "pool_addr": pool, "token_x": "0xa", "token_y": "0xb", "creator": "0xc",
                    "lp_token_supply": "1000000", "reserve_x": "1000000", "reserve_y": "1000000",
                    "platform_fee_percent": "2", "platform_fee_recipient": "0xc",
                    "fee_percent": "1", "created_at": "1"
                }),
            ),
            row(
                base + 1,
                0,
                "dex",
                "LiquidityAddedEvent",
                json!({
                    "pool_address": pool, "provider": "0xd", "token_x": "0xa", "token_y": "0xb",
                    "amount_x": "500", "amount_y": "500", "lp_tokens_minted": "500",
                    "new_reserve_x": "1000500", "new_reserve_y": "1000500", "timestamp": "2"
                }),
            ),
            row(
                base + 2,
                3,
                "dex",
                "SwapEvent",
                json!({
                    "pool_address": pool, "trader": "0xd", "token_in": "0xa", "token_out": "0xb",
                    "amount_in": "100", "amount_out": "99", "swap_fee": "0", "platform_fee": "0",
                    "new_reserve_x": "1000600", "new_reserve_y": "1000401", "timestamp": "3"
                }),
            ),
        ]
    }

    #[tokio::test]
    async fn replaying_a_batch_is_a_no_op() {
        let Some(mut conn) = connect().await else {
            eprintln!("DATABASE_URL not set; skipping");
            return;
        };
        // unique pool + versions per run so reruns don't collide
        let nanos = chrono::Utc::now().timestamp_nanos_opt().unwrap();
        let pool = format!("0x{:x}", nanos);
        let base = nanos / 1000;
        let pool_std = crate::events::standardize_address(&pool);

        assert_eq!(write_batch(&mut conn, batch(&pool, base)).await.unwrap(), 3);
        assert_eq!(write_batch(&mut conn, batch(&pool, base)).await.unwrap(), 0);

        let p: Pool = pools::table
            .find(&pool_std)
            .select(Pool::as_select())
            .first(&mut conn)
            .await
            .unwrap();
        assert_eq!(p.lp_supply, BigDecimal::from(1_000_500));
        assert_eq!(p.reserve_x, BigDecimal::from(1_000_600));
        assert_eq!(p.reserve_y, BigDecimal::from(1_000_401));
        assert_eq!(p.last_version, base + 2);

        let swap_count: i64 = swaps::table
            .filter(swaps::pool.eq(&pool_std))
            .count()
            .get_result(&mut conn)
            .await
            .unwrap();
        assert_eq!(swap_count, 1);
    }
}
