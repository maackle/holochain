//! The workflow and queue consumer for DhtOp integration

use super::*;
use crate::core::queue_consumer::TriggerSender;
use crate::core::queue_consumer::WorkComplete;
use holochain_p2p::HolochainP2pDna;
use holochain_p2p::HolochainP2pDnaT;
use holochain_state::prelude::*;
use holochain_types::projection::polestar_write_op_event;

#[cfg(test)]
mod query_tests;
#[cfg(test)]
mod tests;

#[cfg_attr(
    feature = "instrument",
    tracing::instrument(skip(vault, trigger_receipt, network, dht_query_cache))
)]
pub async fn integrate_dht_ops_workflow(
    vault: DbWrite<DbKindDht>,
    dht_query_cache: DhtDbQueryCache,
    trigger_receipt: TriggerSender,
    network: HolochainP2pDna,
) -> WorkflowResult<WorkComplete> {
    let start = std::time::Instant::now();
    let time = holochain_zome_types::prelude::Timestamp::now();
    // Get any activity from the cache that is ready to be integrated.
    let activity_to_integrate = dht_query_cache.get_activity_to_integrate().await?;
    let tag = network.tag();
    let (changed, activity_integrated) = vault
        .write_async(move |txn| {
            let mut total = 0;
            let mut hashes_to_integrate: Vec<DhtOpHash> = Vec::new();
            if !activity_to_integrate.is_empty() {
                let mut stmt = txn.prepare_cached(
                    holochain_sqlite::sql::sql_cell::UPDATE_INTEGRATE_DEP_ACTIVITY,
                )?;
                for (author, seq_range) in &activity_to_integrate {
                    let start = seq_range.start();
                    let end = seq_range.end();

                    let hashes = stmt
                        .query_and_then(
                            named_params! {
                                ":when_integrated": time,
                                ":register_activity": ChainOpType::RegisterAgentActivity,
                                ":seq_start": start,
                                ":seq_end": end,
                                ":author": author,
                            },
                            |row| row.get("hash"),
                        )?
                        .collect::<Result<Vec<DhtOpHash>, _>>()?;
                    total += hashes.len();
                    hashes_to_integrate.extend(hashes);
                }
            }
            let hashes = txn
                .prepare_cached(holochain_sqlite::sql::sql_cell::UPDATE_INTEGRATE_DEP_STORE_ENTRY)?
                .query_and_then(
                    named_params! {
                        ":when_integrated": time,
                        ":updated_content": ChainOpType::RegisterUpdatedContent,
                        ":deleted_entry_action": ChainOpType::RegisterDeletedEntryAction,
                        ":store_entry": ChainOpType::StoreEntry,
                    },
                    |row| row.get("hash"),
                )?
                .collect::<Result<Vec<DhtOpHash>, _>>()?;
            total += hashes.len();
            hashes_to_integrate.extend(hashes);
            let hashes = txn
                .prepare_cached(
                    holochain_sqlite::sql::sql_cell::UPDATE_INTEGRATE_DEP_STORE_ENTRY_BASIS,
                )?
                .query_and_then(
                    named_params! {
                        ":when_integrated": time,
                        ":create_link": ChainOpType::RegisterAddLink,
                        ":store_entry": ChainOpType::StoreEntry,
                    },
                    |row| row.get("hash"),
                )?
                .collect::<Result<Vec<DhtOpHash>, _>>()?;
            total += hashes.len();
            hashes_to_integrate.extend(hashes);

            let hashes = txn
                .prepare_cached(holochain_sqlite::sql::sql_cell::UPDATE_INTEGRATE_DEP_STORE_RECORD)?
                .query_and_then(
                    named_params! {
                        ":when_integrated": time,
                        ":store_record": ChainOpType::StoreRecord,
                        ":updated_record": ChainOpType::RegisterUpdatedRecord,
                        ":deleted_by": ChainOpType::RegisterDeletedBy,
                    },
                    |row| row.get("hash"),
                )?
                .collect::<Result<Vec<DhtOpHash>, _>>()?;
            total += hashes.len();
            hashes_to_integrate.extend(hashes);

            let hashes = txn
                .prepare_cached(holochain_sqlite::sql::sql_cell::UPDATE_INTEGRATE_DEP_CREATE_LINK)?
                .query_and_then(
                    named_params! {
                        ":when_integrated": time,
                        ":create_link": ChainOpType::RegisterAddLink,
                        ":delete_link": ChainOpType::RegisterRemoveLink,

                    },
                    |row| row.get("hash"),
                )?
                .collect::<Result<Vec<DhtOpHash>, _>>()?;
            total += hashes.len();
            hashes_to_integrate.extend(hashes);

            for hash in hashes_to_integrate {
                polestar_write_op_event(&tag, OpEvent::Integrated { op: hash });
            }

            WorkflowResult::Ok((total, activity_to_integrate))
        })
        .await?;
    // Once the database transaction is committed, update the cache with the
    // integrated activity.
    dht_query_cache
        .set_all_activity_to_integrated(activity_integrated)
        .await?;
    let ops_ps = changed as f64 / start.elapsed().as_micros() as f64 * 1_000_000.0;
    tracing::debug!(?changed, %ops_ps);
    if changed > 0 {
        trigger_receipt.trigger(&"integrate_dht_ops_workflow");
        network.new_integrated_data().await?;
        Ok(WorkComplete::Incomplete(None))
    } else {
        Ok(WorkComplete::Complete)
    }
}
