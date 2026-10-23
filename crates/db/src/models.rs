//! Row types. `New*` structs are written by the processor; the rest are read by the API.
//! Amounts are `BigDecimal` (Postgres NUMERIC) and serialize as JSON strings so u64/u256
//! values survive JavaScript clients without precision loss.

use crate::schema::*;
use bigdecimal::BigDecimal;
use chrono::{DateTime, Utc};
use diesel::prelude::*;
use serde::Serialize;

/// Serialize NUMERIC as a plain decimal string ("1500", not 1.5e3).
pub mod dec {
    use bigdecimal::BigDecimal;
    use serde::Serializer;

    pub fn serialize<S: Serializer>(v: &BigDecimal, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&v.to_plain_string())
    }
}

#[derive(Insertable)]
#[diesel(table_name = dynexus_events)]
pub struct NewEvent<'a> {
    pub transaction_version: i64,
    pub event_index: i64,
    pub module: &'a str,
    pub name: &'a str,
    pub account: Option<&'a str>,
    pub data: &'a serde_json::Value,
    pub ts: DateTime<Utc>,
}

#[derive(Queryable, Selectable, Serialize)]
#[diesel(table_name = dynexus_events)]
pub struct Event {
    pub transaction_version: i64,
    pub event_index: i64,
    pub module: String,
    pub name: String,
    pub account: Option<String>,
    pub data: serde_json::Value,
    pub ts: DateTime<Utc>,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = tokens)]
pub struct Token {
    pub address: String,
    pub creator: String,
    pub name: Option<String>,
    pub symbol: Option<String>,
    pub decimals: Option<i16>,
    pub icon_uri: Option<String>,
    pub project_uri: Option<String>,
    #[serde(with = "dec")]
    pub burned: BigDecimal,
    pub created_at: DateTime<Utc>,
    pub created_version: i64,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = pools)]
pub struct Pool {
    pub address: String,
    pub token_x: String,
    pub token_y: String,
    pub creator: String,
    #[serde(with = "dec")]
    pub reserve_x: BigDecimal,
    #[serde(with = "dec")]
    pub reserve_y: BigDecimal,
    #[serde(with = "dec")]
    pub lp_supply: BigDecimal,
    pub fee_bps: i32,
    pub platform_fee_bps: i32,
    pub created_at: DateTime<Utc>,
    pub created_version: i64,
    pub last_version: i64,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = swaps)]
pub struct Swap {
    pub transaction_version: i64,
    pub event_index: i64,
    pub pool: String,
    pub trader: String,
    pub token_in: String,
    pub token_out: String,
    #[serde(with = "dec")]
    pub amount_in: BigDecimal,
    #[serde(with = "dec")]
    pub amount_out: BigDecimal,
    #[serde(with = "dec")]
    pub swap_fee: BigDecimal,
    #[serde(with = "dec")]
    pub platform_fee: BigDecimal,
    #[serde(with = "dec")]
    pub reserve_x: BigDecimal,
    #[serde(with = "dec")]
    pub reserve_y: BigDecimal,
    pub ts: DateTime<Utc>,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = liquidity_events)]
pub struct LiquidityEvent {
    pub transaction_version: i64,
    pub event_index: i64,
    pub pool: String,
    pub provider: String,
    pub kind: String,
    #[serde(with = "dec")]
    pub amount_x: BigDecimal,
    #[serde(with = "dec")]
    pub amount_y: BigDecimal,
    #[serde(with = "dec")]
    pub lp_amount: BigDecimal,
    #[serde(with = "dec")]
    pub reserve_x: BigDecimal,
    #[serde(with = "dec")]
    pub reserve_y: BigDecimal,
    pub ts: DateTime<Utc>,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = ve_locks)]
pub struct VeLock {
    pub user_address: String,
    #[serde(with = "dec")]
    pub amount: BigDecimal,
    pub unlock_time: i64,
    pub updated_version: i64,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = votes)]
pub struct Vote {
    pub user_address: String,
    pub epoch: i64,
    pub pool: String,
    #[serde(with = "dec")]
    pub vote_weight: BigDecimal,
    pub transaction_version: i64,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = epochs)]
pub struct Epoch {
    pub epoch: i64,
    #[serde(with = "dec")]
    pub emission: BigDecimal,
    #[serde(with = "dec")]
    pub total_votes: BigDecimal,
    pub distributed_version: i64,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = gauges)]
pub struct Gauge {
    pub pool: String,
    pub lp_token: String,
    #[serde(with = "dec")]
    pub total_staked: BigDecimal,
    /// DYNEXUS per second, scaled by 1e18
    #[serde(with = "dec")]
    pub reward_rate: BigDecimal,
    pub period_finish: i64,
    pub created_version: i64,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = gauge_stakes)]
pub struct GaugeStake {
    pub pool: String,
    pub user_address: String,
    #[serde(with = "dec")]
    pub staked: BigDecimal,
}

#[derive(Insertable, Queryable, Selectable, Serialize)]
#[diesel(table_name = bribes)]
pub struct Bribe {
    pub pool: String,
    pub epoch: i64,
    pub token: String,
    #[serde(with = "dec")]
    pub amount: BigDecimal,
}
