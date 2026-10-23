use crate::DbPool;
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use bigdecimal::BigDecimal;
use chrono::{DateTime, Duration, Utc};
use diesel::{
    dsl::{count_star, sum},
    prelude::*,
};
use diesel_async::RunQueryDsl;
use dynexus_db::{models::*, schema::*};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_LIMIT: i64 = 200;
const WEEK_SECS: i64 = 604_800;

pub fn router(pool: DbPool) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/tokens", get(list_tokens))
        .route("/tokens/{address}", get(get_token))
        .route("/pools", get(list_pools))
        .route("/pools/{address}", get(get_pool))
        .route("/pools/{address}/swaps", get(pool_swaps))
        .route("/pools/{address}/liquidity", get(pool_liquidity))
        .route("/pools/{address}/candles", get(pool_candles))
        .route("/gauges", get(list_gauges))
        .route("/epochs/{epoch}", get(get_epoch))
        .route("/users/{address}/activity", get(user_activity))
        .route("/users/{address}/governance", get(user_governance))
        .with_state(pool)
}

//
// Errors & helpers
//

#[derive(Debug)]
pub enum ApiError {
    NotFound,
    BadRequest(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not found".to_string()),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Internal(m) => {
                tracing::error!(error = %m, "request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".to_string())
            }
        };
        (status, Json(json!({ "error": msg }))).into_response()
    }
}

impl From<diesel::result::Error> for ApiError {
    fn from(e: diesel::result::Error) -> Self {
        match e {
            diesel::result::Error::NotFound => ApiError::NotFound,
            other => ApiError::Internal(other.to_string()),
        }
    }
}

type ApiResult<T> = Result<T, ApiError>;
type Conn<'a> = diesel_async::pooled_connection::bb8::PooledConnection<'a, diesel_async::AsyncPgConnection>;

async fn conn(pool: &DbPool) -> ApiResult<Conn<'_>> {
    pool.get().await.map_err(|e| ApiError::Internal(e.to_string()))
}

/// Short public cache: data changes at most once per block.
fn cached<T: Serialize>(body: T) -> Response {
    let mut res = Json(body).into_response();
    res.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=5"),
    );
    res
}

fn address(raw: &str) -> ApiResult<String> {
    let hex = raw.trim_start_matches("0x");
    if hex.is_empty() || hex.len() > 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ApiError::BadRequest(format!("invalid address {raw:?}")));
    }
    Ok(format!("0x{:0>64}", hex.to_ascii_lowercase()))
}

#[derive(Deserialize)]
pub struct Page {
    limit: Option<i64>,
    offset: Option<i64>,
}

impl Page {
    fn limit(&self) -> i64 {
        self.limit.unwrap_or(50).clamp(1, MAX_LIMIT)
    }
    fn offset(&self) -> i64 {
        self.offset.unwrap_or(0).max(0)
    }
}

fn dec_str(d: Option<BigDecimal>) -> String {
    d.unwrap_or_default().to_plain_string()
}

//
// Handlers
//

async fn health(State(pool): State<DbPool>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let latest: Option<i64> = dynexus_events::table
        .select(diesel::dsl::max(dynexus_events::transaction_version))
        .first(&mut c)
        .await?;
    Ok(Json(json!({ "ok": true, "latest_version": latest })).into_response())
}

async fn stats(State(pool): State<DbPool>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let since = Utc::now() - Duration::hours(24);
    let tokens: i64 = tokens::table.select(count_star()).first(&mut c).await?;
    let pools: i64 = pools::table.select(count_star()).first(&mut c).await?;
    let swaps_24h: i64 = swaps::table
        .filter(swaps::ts.ge(since))
        .select(count_star())
        .first(&mut c)
        .await?;
    let traders_24h: i64 = swaps::table
        .filter(swaps::ts.ge(since))
        .select(diesel::dsl::count(swaps::trader).aggregate_distinct())
        .first(&mut c)
        .await?;
    let total_locked: Option<BigDecimal> = ve_locks::table
        .select(sum(ve_locks::amount))
        .first(&mut c)
        .await?;
    let lockers: i64 = ve_locks::table.select(count_star()).first(&mut c).await?;
    Ok(cached(json!({
        "tokens": tokens,
        "pools": pools,
        "swaps_24h": swaps_24h,
        "traders_24h": traders_24h,
        "ve_total_locked": dec_str(total_locked),
        "ve_lockers": lockers,
        "current_epoch": Utc::now().timestamp() / WEEK_SECS,
    })))
}

#[derive(Deserialize)]
pub struct TokenQuery {
    creator: Option<String>,
    q: Option<String>,
    #[serde(flatten)]
    page: Page,
}

async fn list_tokens(State(pool): State<DbPool>, Query(q): Query<TokenQuery>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let mut query = tokens::table.select(Token::as_select()).into_boxed();
    if let Some(creator) = &q.creator {
        query = query.filter(tokens::creator.eq(address(creator)?));
    }
    if let Some(term) = q.q.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        let escaped = term.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
        let pattern = format!("%{escaped}%");
        query = query.filter(
            tokens::name
                .ilike(pattern.clone())
                .or(tokens::symbol.ilike(pattern)),
        );
    }
    let rows: Vec<Token> = query
        .order(tokens::created_version.desc())
        .limit(q.page.limit())
        .offset(q.page.offset())
        .load(&mut c)
        .await?;
    Ok(cached(rows))
}

async fn get_token(State(pool): State<DbPool>, Path(addr): Path<String>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let addr = address(&addr)?;
    let token: Token = tokens::table
        .find(&addr)
        .select(Token::as_select())
        .first(&mut c)
        .await?;
    let pools: Vec<String> = pools::table
        .filter(pools::token_x.eq(&addr).or(pools::token_y.eq(&addr)))
        .select(pools::address)
        .load(&mut c)
        .await?;
    Ok(cached(json!({ "token": token, "pools": pools })))
}

#[derive(Serialize)]
struct PoolView {
    #[serde(flatten)]
    pool: Pool,
    token_x_symbol: Option<String>,
    token_y_symbol: Option<String>,
    token_x_decimals: Option<i16>,
    token_y_decimals: Option<i16>,
    swaps_24h: i64,
    /// Sum of `amount_in` over 24h, per input token (base units)
    volume_x_24h: String,
    volume_y_24h: String,
    has_gauge: bool,
}

async fn pool_views(c: &mut Conn<'_>, rows: Vec<Pool>) -> ApiResult<Vec<PoolView>> {
    let since = Utc::now() - Duration::hours(24);
    let token_addrs: Vec<&String> = rows.iter().flat_map(|p| [&p.token_x, &p.token_y]).collect();
    let metas: Vec<(String, Option<String>, Option<i16>)> = tokens::table
        .filter(tokens::address.eq_any(token_addrs))
        .select((tokens::address, tokens::symbol, tokens::decimals))
        .load(c)
        .await?;
    let meta = |a: &str| metas.iter().find(|m| m.0 == a).map(|m| (m.1.clone(), m.2));
    let pool_addrs: Vec<&String> = rows.iter().map(|p| &p.address).collect();
    let volumes: Vec<(String, String, i64, Option<BigDecimal>)> = swaps::table
        .filter(swaps::pool.eq_any(&pool_addrs))
        .filter(swaps::ts.ge(since))
        .group_by((swaps::pool, swaps::token_in))
        .select((swaps::pool, swaps::token_in, count_star(), sum(swaps::amount_in)))
        .load(c)
        .await?;
    let gauge_pools: Vec<String> = gauges::table
        .filter(gauges::pool.eq_any(&pool_addrs))
        .select(gauges::pool)
        .load(c)
        .await?;

    Ok(rows
        .into_iter()
        .map(|pool| {
            let (xs, xd) = meta(&pool.token_x).unwrap_or_default();
            let (ys, yd) = meta(&pool.token_y).unwrap_or_default();
            let vol = |token: &str| {
                volumes
                    .iter()
                    .find(|v| v.0 == pool.address && v.1 == token)
                    .cloned()
            };
            let (vx, vy) = (vol(&pool.token_x), vol(&pool.token_y));
            PoolView {
                swaps_24h: vx.as_ref().map_or(0, |v| v.2) + vy.as_ref().map_or(0, |v| v.2),
                volume_x_24h: dec_str(vx.and_then(|v| v.3)),
                volume_y_24h: dec_str(vy.and_then(|v| v.3)),
                has_gauge: gauge_pools.contains(&pool.address),
                token_x_symbol: xs,
                token_y_symbol: ys,
                token_x_decimals: xd,
                token_y_decimals: yd,
                pool,
            }
        })
        .collect())
}

#[derive(Deserialize)]
pub struct PoolQuery {
    token: Option<String>,
    #[serde(flatten)]
    page: Page,
}

async fn list_pools(State(pool): State<DbPool>, Query(q): Query<PoolQuery>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let mut query = pools::table.select(Pool::as_select()).into_boxed();
    if let Some(token) = &q.token {
        let token = address(token)?;
        query = query.filter(pools::token_x.eq(token.clone()).or(pools::token_y.eq(token)));
    }
    let rows: Vec<Pool> = query
        .order(pools::created_version.desc())
        .limit(q.page.limit())
        .offset(q.page.offset())
        .load(&mut c)
        .await?;
    Ok(cached(pool_views(&mut c, rows).await?))
}

async fn get_pool(State(pool): State<DbPool>, Path(addr): Path<String>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let row: Pool = pools::table
        .find(address(&addr)?)
        .select(Pool::as_select())
        .first(&mut c)
        .await?;
    let mut views = pool_views(&mut c, vec![row]).await?;
    Ok(cached(views.pop()))
}

async fn pool_swaps(
    State(pool): State<DbPool>,
    Path(addr): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let rows: Vec<Swap> = swaps::table
        .filter(swaps::pool.eq(address(&addr)?))
        .order((swaps::transaction_version.desc(), swaps::event_index.desc()))
        .limit(page.limit())
        .offset(page.offset())
        .select(Swap::as_select())
        .load(&mut c)
        .await?;
    Ok(cached(rows))
}

async fn pool_liquidity(
    State(pool): State<DbPool>,
    Path(addr): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let rows: Vec<LiquidityEvent> = liquidity_events::table
        .filter(liquidity_events::pool.eq(address(&addr)?))
        .order((
            liquidity_events::transaction_version.desc(),
            liquidity_events::event_index.desc(),
        ))
        .limit(page.limit())
        .offset(page.offset())
        .select(LiquidityEvent::as_select())
        .load(&mut c)
        .await?;
    Ok(cached(rows))
}

#[derive(Deserialize)]
pub struct CandleQuery {
    /// Bucket size in seconds (60..=86400), default 3600
    interval: Option<i64>,
    /// Number of buckets, default 48
    limit: Option<i64>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Candle {
    pub start: i64,
    /// Price of token_x in token_y (reserve_y / reserve_x after each swap)
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub swaps: usize,
}

/// Build OHLC candles from post-swap reserves `(unix_ts, reserve_x, reserve_y)`, oldest first.
pub fn build_candles(points: &[(i64, f64, f64)], interval: i64) -> Vec<Candle> {
    let mut out: Vec<Candle> = Vec::new();
    for &(ts, rx, ry) in points {
        if rx <= 0.0 {
            continue;
        }
        let price = ry / rx;
        let start = ts - ts.rem_euclid(interval);
        match out.last_mut() {
            Some(c) if c.start == start => {
                c.high = c.high.max(price);
                c.low = c.low.min(price);
                c.close = price;
                c.swaps += 1;
            }
            _ => out.push(Candle {
                start,
                open: price,
                high: price,
                low: price,
                close: price,
                swaps: 1,
            }),
        }
    }
    out
}

async fn pool_candles(
    State(pool): State<DbPool>,
    Path(addr): Path<String>,
    Query(q): Query<CandleQuery>,
) -> ApiResult<Response> {
    let interval = q.interval.unwrap_or(3600).clamp(60, 86_400);
    let buckets = q.limit.unwrap_or(48).clamp(1, 500);
    let since = Utc::now() - Duration::seconds(interval * buckets);
    let mut c = conn(&pool).await?;
    let rows: Vec<(DateTime<Utc>, BigDecimal, BigDecimal)> = swaps::table
        .filter(swaps::pool.eq(address(&addr)?))
        .filter(swaps::ts.ge(since))
        .order((swaps::transaction_version.asc(), swaps::event_index.asc()))
        .select((swaps::ts, swaps::reserve_x, swaps::reserve_y))
        .load(&mut c)
        .await?;
    // f64 is fine for charting; exact values stay available via /swaps.
    let points: Vec<(i64, f64, f64)> = rows
        .iter()
        .map(|(ts, rx, ry)| {
            (
                ts.timestamp(),
                rx.to_plain_string().parse().unwrap_or(0.0),
                ry.to_plain_string().parse().unwrap_or(0.0),
            )
        })
        .collect();
    Ok(cached(build_candles(&points, interval)))
}

async fn list_gauges(State(pool): State<DbPool>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let epoch = Utc::now().timestamp() / WEEK_SECS;
    let rows: Vec<Gauge> = gauges::table
        .order(gauges::created_version.asc())
        .select(Gauge::as_select())
        .load(&mut c)
        .await?;
    let epoch_votes: Vec<(String, Option<BigDecimal>)> = votes::table
        .filter(votes::epoch.eq(epoch))
        .group_by(votes::pool)
        .select((votes::pool, sum(votes::vote_weight)))
        .load(&mut c)
        .await?;
    let bribes_now: Vec<Bribe> = bribes::table
        .filter(bribes::epoch.eq(epoch))
        .select(Bribe::as_select())
        .load(&mut c)
        .await?;
    let out: Vec<Value> = rows
        .into_iter()
        .map(|g| {
            let votes = epoch_votes
                .iter()
                .find(|v| v.0 == g.pool)
                .and_then(|v| v.1.clone());
            let bribes: Vec<&Bribe> = bribes_now.iter().filter(|b| b.pool == g.pool).collect();
            json!({ "gauge": g, "epoch": epoch, "votes": dec_str(votes), "bribes": bribes })
        })
        .collect();
    Ok(cached(out))
}

async fn get_epoch(State(pool): State<DbPool>, Path(epoch): Path<i64>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let distributed: Option<Epoch> = epochs::table
        .find(epoch)
        .select(Epoch::as_select())
        .first(&mut c)
        .await
        .optional()?;
    let pool_votes: Vec<(String, Option<BigDecimal>)> = votes::table
        .filter(votes::epoch.eq(epoch))
        .group_by(votes::pool)
        .select((votes::pool, sum(votes::vote_weight)))
        .load(&mut c)
        .await?;
    let bribes: Vec<Bribe> = bribes::table
        .filter(bribes::epoch.eq(epoch))
        .select(Bribe::as_select())
        .load(&mut c)
        .await?;
    let pool_votes: Vec<Value> = pool_votes
        .into_iter()
        .map(|(p, v)| json!({ "pool": p, "votes": dec_str(v) }))
        .collect();
    Ok(cached(json!({
        "epoch": epoch,
        "starts_at": epoch * WEEK_SECS,
        "ends_at": (epoch + 1) * WEEK_SECS,
        "distribution": distributed,
        "pool_votes": pool_votes,
        "bribes": bribes,
    })))
}

async fn user_activity(
    State(pool): State<DbPool>,
    Path(addr): Path<String>,
    Query(page): Query<Page>,
) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let rows: Vec<Event> = dynexus_events::table
        .filter(dynexus_events::account.eq(address(&addr)?))
        .order((
            dynexus_events::transaction_version.desc(),
            dynexus_events::event_index.desc(),
        ))
        .limit(page.limit())
        .offset(page.offset())
        .select(Event::as_select())
        .load(&mut c)
        .await?;
    Ok(cached(rows))
}

async fn user_governance(State(pool): State<DbPool>, Path(addr): Path<String>) -> ApiResult<Response> {
    let mut c = conn(&pool).await?;
    let user = address(&addr)?;
    let epoch = Utc::now().timestamp() / WEEK_SECS;
    let lock: Option<VeLock> = ve_locks::table
        .find(&user)
        .select(VeLock::as_select())
        .first(&mut c)
        .await
        .optional()?;
    let votes: Vec<Vote> = votes::table
        .filter(votes::user_address.eq(&user))
        .filter(votes::epoch.ge(epoch - 8))
        .order(votes::epoch.desc())
        .select(Vote::as_select())
        .load(&mut c)
        .await?;
    let stakes: Vec<GaugeStake> = gauge_stakes::table
        .filter(gauge_stakes::user_address.eq(&user))
        .filter(gauge_stakes::staked.gt(BigDecimal::from(0)))
        .select(GaugeStake::as_select())
        .load(&mut c)
        .await?;
    Ok(cached(
        json!({ "epoch": epoch, "lock": lock, "votes": votes, "stakes": stakes }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn address_is_standardized_and_validated() {
        assert_eq!(address("0xA").unwrap(), format!("0x{}a", "0".repeat(63)));
        assert!(address("0xzz").is_err());
        assert!(address("").is_err());
        assert!(address(&format!("0x{}", "1".repeat(65))).is_err());
    }

    #[test]
    fn page_limits_are_clamped() {
        let p = Page {
            limit: Some(10_000),
            offset: Some(-5),
        };
        assert_eq!((p.limit(), p.offset()), (MAX_LIMIT, 0));
        let p = Page {
            limit: None,
            offset: None,
        };
        assert_eq!((p.limit(), p.offset()), (50, 0));
    }

    #[test]
    fn candles_bucket_prices() {
        let pts = [
            (3600, 100.0, 200.0),
            (3700, 100.0, 300.0),
            (3800, 100.0, 150.0),
            (7300, 50.0, 50.0),
        ];
        let candles = build_candles(&pts, 3600);
        assert_eq!(candles.len(), 2);
        assert_eq!(
            candles[0],
            Candle {
                start: 3600,
                open: 2.0,
                high: 3.0,
                low: 1.5,
                close: 1.5,
                swaps: 3
            }
        );
        assert_eq!(candles[1].start, 7200);
        assert_eq!(candles[1].close, 1.0);
    }

    #[test]
    fn candles_skip_empty_reserves() {
        assert!(build_candles(&[(0, 0.0, 10.0)], 60).is_empty());
    }
}
