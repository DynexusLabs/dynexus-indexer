//! Decoding of DYNEXUS Move events into typed Rust values.
//!
//! Everything here is pure (no I/O) so it can be unit-tested against JSON captured
//! from the chain. Move `u64`/`u128`/`u256` values arrive as JSON strings and are kept
//! as validated decimal strings ([`Num`]) so they can be stored as Postgres `NUMERIC`
//! without precision loss.

use serde::{Deserialize, Deserializer, de::Error as _};
use std::collections::HashSet;

/// Canonical `0x` + 64 lowercase hex form, matching the SDK's `standardize_address`.
pub fn standardize_address(addr: &str) -> String {
    let hex = addr.trim_start_matches("0x").to_ascii_lowercase();
    format!("0x{hex:0>64}")
}

/// Decimal unsigned integer of arbitrary width, as emitted by Move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Num(pub String);

impl<'de> Deserialize<'de> for Num {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let v = serde_json::Value::deserialize(d)?;
        let s = match v {
            serde_json::Value::String(s) => s,
            serde_json::Value::Number(n) => n.to_string(),
            other => return Err(D::Error::custom(format!("expected number, got {other}"))),
        };
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(D::Error::custom(format!("invalid unsigned integer {s:?}")));
        }
        Ok(Num(s))
    }
}

impl Num {
    /// For values that are bounded by protocol rules (epochs, timestamps, bps).
    pub fn as_i64(&self) -> Result<i64, std::num::ParseIntError> {
        self.0.parse()
    }
}

/// Account/object address, standardized on decode.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Addr(pub String);

impl<'de> Deserialize<'de> for Addr {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        if !s.starts_with("0x") || s.len() > 66 || !s[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(D::Error::custom(format!("invalid address {s:?}")));
        }
        Ok(Addr(standardize_address(&s)))
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct TokenCreated {
    pub creator: Addr,
    pub token_address: Addr,
    pub timestamp: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct TokenBurned {
    pub owner: Addr,
    pub token_address: Addr,
    pub amount: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct PoolCreated {
    pub pool_addr: Addr,
    pub token_x: Addr,
    pub token_y: Addr,
    pub creator: Addr,
    pub lp_token_supply: Num,
    pub reserve_x: Num,
    pub reserve_y: Num,
    pub platform_fee_percent: Num,
    pub fee_percent: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Swap {
    pub pool_address: Addr,
    pub trader: Addr,
    pub token_in: Addr,
    pub token_out: Addr,
    pub amount_in: Num,
    pub amount_out: Num,
    pub swap_fee: Num,
    pub platform_fee: Num,
    pub new_reserve_x: Num,
    pub new_reserve_y: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct LiquidityAdded {
    pub pool_address: Addr,
    pub provider: Addr,
    pub amount_x: Num,
    pub amount_y: Num,
    pub lp_tokens_minted: Num,
    pub new_reserve_x: Num,
    pub new_reserve_y: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct LiquidityRemoved {
    pub pool_address: Addr,
    pub provider: Addr,
    pub amount_x: Num,
    pub amount_y: Num,
    pub lp_tokens_burned: Num,
    pub new_reserve_x: Num,
    pub new_reserve_y: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct FeeConfigUpdated {
    pub default_fee_percent: Num,
    pub platform_fee_percent: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct LockUpdated {
    pub user: Addr,
    pub amount: Num,
    pub unlock_time: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Withdrawn {
    pub user: Addr,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Voted {
    pub user: Addr,
    pub epoch: Num,
    pub pools: Vec<Addr>,
    pub votes: Vec<Num>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct VotesReset {
    pub user: Addr,
    pub epoch: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct EpochDistributed {
    pub epoch: Num,
    pub emission: Num,
    pub total_votes: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct GaugeCreated {
    pub pool: Addr,
    pub lp_token: Addr,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct StakeChanged {
    pub user: Addr,
    pub pool: Addr,
    pub amount: Num,
    pub total_staked: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct RewardNotified {
    pub pool: Addr,
    pub reward_rate: Num,
    pub period_finish: Num,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct BribeAdded {
    pub pool: Addr,
    pub epoch: Num,
    pub token: Addr,
    pub amount: Num,
}

/// A decoded DYNEXUS event. Events without a typed table (faucet claims, reward and
/// bribe claims, emission schedule, minter changes) are kept only in `dynexus_events`.
#[derive(Debug, PartialEq)]
pub enum DynexusEvent {
    TokenCreated(TokenCreated),
    TokenBurned(TokenBurned),
    PoolCreated(PoolCreated),
    Swap(Swap),
    LiquidityAdded(LiquidityAdded),
    LiquidityRemoved(LiquidityRemoved),
    FeeConfigUpdated(FeeConfigUpdated),
    LockUpdated(LockUpdated),
    Withdrawn(Withdrawn),
    Voted(Voted),
    VotesReset(VotesReset),
    EpochDistributed(EpochDistributed),
    GaugeCreated(GaugeCreated),
    Staked(StakeChanged),
    Unstaked(StakeChanged),
    RewardNotified(RewardNotified),
    BribeAdded(BribeAdded),
    Other,
}

/// `(module, name)` of an event type string if it belongs to one of `addresses`.
/// Generic types (`...::Foo<T>`) are not emitted by DYNEXUS and are rejected.
pub fn match_type<'a>(type_str: &'a str, addresses: &HashSet<String>) -> Option<(&'a str, &'a str)> {
    let mut parts = type_str.splitn(3, "::");
    let (addr, module, name) = (parts.next()?, parts.next()?, parts.next()?);
    if name.contains('<') || !addresses.contains(&standardize_address(addr)) {
        return None;
    }
    Some((module, name))
}

pub fn decode(module: &str, name: &str, data: &serde_json::Value) -> serde_json::Result<DynexusEvent> {
    use DynexusEvent as E;
    let d = || data.clone();
    Ok(match (module, name) {
        ("token_factory", "TokenCreated") => E::TokenCreated(serde_json::from_value(d())?),
        ("token_factory", "TokenBurned") => E::TokenBurned(serde_json::from_value(d())?),
        ("dex", "PoolCratedEvent") => E::PoolCreated(serde_json::from_value(d())?),
        ("dex", "SwapEvent") => E::Swap(serde_json::from_value(d())?),
        ("dex", "LiquidityAddedEvent") => E::LiquidityAdded(serde_json::from_value(d())?),
        ("dex", "LiquidityRemovedEvent") => E::LiquidityRemoved(serde_json::from_value(d())?),
        ("dex", "FeeConfigUpdatedEvent") => E::FeeConfigUpdated(serde_json::from_value(d())?),
        ("voting_escrow", "LockUpdated") => E::LockUpdated(serde_json::from_value(d())?),
        ("voting_escrow", "Withdrawn") => E::Withdrawn(serde_json::from_value(d())?),
        ("voter", "Voted") => E::Voted(serde_json::from_value(d())?),
        ("voter", "VotesReset") => E::VotesReset(serde_json::from_value(d())?),
        ("voter", "EpochDistributed") => E::EpochDistributed(serde_json::from_value(d())?),
        ("gauge", "GaugeCreated") => E::GaugeCreated(serde_json::from_value(d())?),
        ("gauge", "Staked") => E::Staked(serde_json::from_value(d())?),
        ("gauge", "Unstaked") => E::Unstaked(serde_json::from_value(d())?),
        ("gauge", "RewardNotified") => E::RewardNotified(serde_json::from_value(d())?),
        ("bribe", "BribeAdded") => E::BribeAdded(serde_json::from_value(d())?),
        _ => E::Other,
    })
}

/// Primary account an event concerns, for per-user activity feeds.
pub fn actor(data: &serde_json::Value) -> Option<String> {
    ["user", "trader", "provider", "creator", "owner", "briber"]
        .iter()
        .find_map(|k| data.get(*k)?.as_str())
        .map(standardize_address)
}

/// Token metadata read from a `0x1::fungible_asset::Metadata` write in the same transaction.
#[derive(Debug, Deserialize, PartialEq)]
pub struct FaMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    pub icon_uri: String,
    pub project_uri: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const ADDR: &str = "0x6e9d623d8aa35cf0453f96e2590d782bd19181e3f63758da443003b6049c44f6";

    fn addrs() -> HashSet<String> {
        HashSet::from([ADDR.to_string()])
    }

    #[test]
    fn standardizes_short_addresses() {
        assert_eq!(standardize_address("0xA"), format!("0x{}a", "0".repeat(63)));
        assert_eq!(standardize_address(ADDR), ADDR);
    }

    #[test]
    fn matches_only_configured_addresses() {
        let t = format!("{ADDR}::dex::SwapEvent");
        assert_eq!(match_type(&t, &addrs()), Some(("dex", "SwapEvent")));
        assert_eq!(match_type("0x1::coin::CoinDeposit", &addrs()), None);
        let short = "0x6e9d623d8aa35cf0453f96e2590d782bd19181e3f63758da443003b6049c44f6::voter::Voted";
        assert!(match_type(short, &addrs()).is_some());
        assert_eq!(match_type(&format!("{ADDR}::dex::Foo<u8>"), &addrs()), None);
        assert_eq!(match_type("garbage", &addrs()), None);
    }

    #[test]
    fn decodes_swap_with_string_numbers() {
        let data = json!({
            "pool_address": "0xabc", "trader": "0x1", "token_in": "0x2", "token_out": "0x3",
            "amount_in": "18446744073709551615", "amount_out": "5", "swap_fee": "0",
            "platform_fee": "1", "new_reserve_x": "100", "new_reserve_y": "200", "timestamp": "9"
        });
        let DynexusEvent::Swap(s) = decode("dex", "SwapEvent", &data).unwrap() else {
            panic!("not a swap")
        };
        assert_eq!(s.amount_in, Num("18446744073709551615".into()));
        assert_eq!(s.pool_address, Addr(standardize_address("0xabc")));
    }

    #[test]
    fn rejects_malformed_numbers_and_addresses() {
        let bad_num = json!({"user": "0x1", "amount": "-1", "unlock_time": "0"});
        assert!(decode("voting_escrow", "LockUpdated", &bad_num).is_err());
        let bad_addr = json!({"user": "zz", "amount": "1", "unlock_time": "0"});
        assert!(decode("voting_escrow", "LockUpdated", &bad_addr).is_err());
    }

    #[test]
    fn decodes_votes_and_u256_reward_rate() {
        let voted = json!({
            "user": "0x1", "epoch": "2931", "pools": ["0xa", "0xb"],
            "votes": ["10", "20"], "voting_power": "30"
        });
        let DynexusEvent::Voted(v) = decode("voter", "Voted", &voted).unwrap() else {
            panic!("not a vote")
        };
        assert_eq!(v.pools.len(), 2);
        assert_eq!(v.epoch.as_i64().unwrap(), 2931);

        let notified = json!({
            "pool": "0xa", "amount": "1",
            "reward_rate": "115792089237316195423570985008687907853269984665640564039457584007913129639935",
            "period_finish": "1"
        });
        assert!(matches!(
            decode("gauge", "RewardNotified", &notified).unwrap(),
            DynexusEvent::RewardNotified(_)
        ));
    }

    #[test]
    fn unknown_events_are_other() {
        assert_eq!(
            decode("minter", "EpochEmission", &json!({})).unwrap(),
            DynexusEvent::Other
        );
    }

    #[test]
    fn actor_prefers_user_fields() {
        assert_eq!(
            actor(&json!({"trader": "0x2", "pool_address": "0x3"})),
            Some(standardize_address("0x2"))
        );
        assert_eq!(actor(&json!({"epoch": "1"})), None);
    }
}
