//! DYNEXUS event processor.
//!
//! Streams transactions from the Aptos Transaction Stream (gRPC) using the Aptos indexer
//! SDK, which handles reconnection, checkpointing and migrations. Configure a server-side
//! `transaction_filter` (see `config/processor.example.yaml`) so only transactions that
//! emit DYNEXUS events are streamed. Token metadata is read from each transaction's write
//! set, so indexing needs no extra RPC calls.

mod events;
mod store;

use anyhow::{Context, Result, bail};
use aptos_indexer_processor_sdk::{
    aptos_protos::transaction::v1::{Transaction, transaction::TxnData, write_set_change::Change},
    postgres::{basic_processor::process, utils::database::ArcDbPool},
    utils::errors::ProcessorError,
};
use dynexus_db::MIGRATIONS;
use events::{DynexusEvent, FaMetadata};
use std::{collections::HashSet, sync::Arc};
use store::EventRow;
use tracing::{info, warn};

const PROCESSOR_NAME: &str = "dynexus_processor";
const FA_METADATA_TYPE: &str = "0x1::fungible_asset::Metadata";

/// Comma-separated DYNEXUS package addresses (forge, swap, core may share one).
fn addresses_from_env() -> Result<HashSet<String>> {
    let raw = std::env::var("DYNEXUS_ADDRESSES").context("DYNEXUS_ADDRESSES must be set")?;
    let set: HashSet<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(events::standardize_address)
        .collect();
    if set.is_empty() {
        bail!("DYNEXUS_ADDRESSES is empty");
    }
    Ok(set)
}

fn token_metadata(txn: &Transaction, token: &str) -> Option<FaMetadata> {
    txn.info
        .as_ref()?
        .changes
        .iter()
        .find_map(|change| match change.change.as_ref()? {
            Change::WriteResource(r)
                if r.type_str == FA_METADATA_TYPE && events::standardize_address(&r.address) == token =>
            {
                serde_json::from_str(&r.data).ok()
            }
            _ => None,
        })
}

fn extract(txn: &Transaction, addresses: &HashSet<String>) -> Vec<EventRow> {
    let Some(TxnData::User(user)) = txn.txn_data.as_ref() else {
        return vec![];
    };
    let version = txn.version as i64;
    let ts = txn.timestamp.as_ref().map(|t| t.seconds).unwrap_or_default();
    user.events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| {
            let (module, name) = events::match_type(&event.type_str, addresses)?;
            let data: serde_json::Value = match serde_json::from_str(&event.data) {
                Ok(d) => d,
                Err(e) => {
                    warn!(version, index, error = %e, "event data is not JSON; skipping");
                    return None;
                }
            };
            let decoded = events::decode(module, name, &data).unwrap_or_else(|e| {
                // Keep the raw row so nothing is silently lost; typed tables skip it.
                warn!(version, index, module, name, error = %e, "undecodable DYNEXUS event");
                DynexusEvent::Other
            });
            let token_metadata = match &decoded {
                DynexusEvent::TokenCreated(e) => token_metadata(txn, &e.token_address.0),
                _ => None,
            };
            Some(EventRow {
                version,
                index: index as i64,
                module: module.to_string(),
                name: name.to_string(),
                account: events::actor(&data),
                data,
                ts,
                event: decoded,
                token_metadata,
            })
        })
        .collect()
}

async fn handle_batch(
    addresses: Arc<HashSet<String>>,
    transactions: Vec<Transaction>,
    pool: ArcDbPool,
) -> Result<(), ProcessorError> {
    let (Some(first), Some(last)) = (transactions.first(), transactions.last()) else {
        return Ok(());
    };
    let (first, last) = (first.version, last.version);
    let rows: Vec<EventRow> = transactions.iter().flat_map(|t| extract(t, &addresses)).collect();
    if rows.is_empty() {
        return Ok(());
    }
    let total = rows.len();
    let mut conn = pool.get().await.map_err(|e| ProcessorError::DBStoreError {
        message: format!("failed to get connection: {e}"),
        query: None,
    })?;
    let applied = store::write_batch(&mut conn, rows)
        .await
        .map_err(|e| ProcessorError::DBStoreError {
            message: format!("versions [{first}, {last}]: {e}"),
            query: None,
        })?;
    info!(first, last, total, applied, "stored DYNEXUS events");
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let addresses = Arc::new(addresses_from_env()?);
    process(
        PROCESSOR_NAME.to_string(),
        MIGRATIONS,
        move |transactions, pool| handle_batch(addresses.clone(), transactions, pool),
    )
    .await
}
