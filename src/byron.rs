use crate::perf_aggregator::PerfAggregator;
use crate::relation_map::RelationMap;
use cryptoxide::blake2b::Blake2b;
use entity::{
    prelude::*,
    sea_orm::{prelude::*, DatabaseTransaction, Set},
};
use pallas::ledger::primitives::{
    byron::{self, TxIn, TxOut},
    Fragment,
};

pub async fn process_byron_block(
    perf_aggregator: &mut PerfAggregator,
    time_counter: &mut std::time::Instant,
    txn: &DatabaseTransaction,
    db_block: &BlockModel,
    byron_block: &byron::Block,
) -> Result<(), DbErr> {
    match byron_block {
        // Byron era had Epoch-boundary blocks for calculating stake distribution changes
        // they don't contain any txs, so we can just ignore them
        byron::Block::EbBlock(_) => (),
        byron::Block::MainBlock(main_block) => {
            for (idx, tx_body) in main_block.body.tx_payload.iter().enumerate() {
                let tx_hash = blake2b256(&tx_body.transaction.encode_fragment().expect(""));

                let tx_payload = tx_body.encode_fragment().unwrap();

                let transaction = TransactionActiveModel {
                    hash: Set(tx_hash.to_vec()),
                    block_id: Set(db_block.id),
                    tx_index: Set(idx as i32),
                    payload: Set(tx_payload),
                    is_valid: Set(true), // always true in Byron
                    ..Default::default()
                };

                let transaction = transaction.insert(txn).await?;

                // unused for Byron
                let mut vkey_relation_map = RelationMap::default();

                perf_aggregator.transaction_insert += time_counter.elapsed();
                *time_counter = std::time::Instant::now();

                // note: outputs have to be added before inputs
                insert_byron_outputs(txn, &transaction, &tx_body.transaction.outputs).await?;

                perf_aggregator.transaction_output_insert += time_counter.elapsed();
                *time_counter = std::time::Instant::now();

                let inputs = tx_body
                    .transaction
                    .inputs
                    .iter()
                    .map(|input| byron_input_to_alonzo(&input))
                    .collect();

                crate::era_common::insert_inputs(
                    &mut vkey_relation_map,
                    transaction.id,
                    &inputs,
                    txn,
                )
                .await?;

                perf_aggregator.transaction_input_insert += time_counter.elapsed();
                *time_counter = std::time::Instant::now();
            }
        }
    }

    Ok(())
}

async fn insert_byron_outputs(
    txn: &DatabaseTransaction,
    transaction: &TransactionModel,
    outputs: &Vec<TxOut>,
) -> Result<(), DbErr> {
    let address_inserts = crate::era_common::insert_addresses(
        &outputs
            .iter()
            .map(|output| output.address.encode_fragment().unwrap())
            .collect(),
        txn,
    )
    .await?;

    TransactionOutput::insert_many(outputs.iter().enumerate().map(|(idx, output)| {
        TransactionOutputActiveModel {
            payload: Set(output.encode_fragment().unwrap()),
            address_id: Set(address_inserts.get(idx).unwrap().id),
            tx_id: Set(transaction.id),
            output_index: Set(idx as i32),
            ..Default::default()
        }
    }))
    .exec(txn)
    .await?;

    Ok(())
}

fn byron_input_to_alonzo(input: &TxIn) -> pallas::ledger::primitives::alonzo::TransactionInput {
    match input {
        TxIn::Variant0(wrapped) => pallas::ledger::primitives::alonzo::TransactionInput {
            transaction_id: wrapped.0 .0.clone(),
            index: wrapped.0 .1 as u64,
        },
        TxIn::Other(index, tx_hash) => {
            // Note: Oura uses "other" to future proof itself against changes in the binary spec
            todo!("handle TxIn::Other({:?}, {:?})", index, tx_hash)
        }
    }
}

fn blake2b256(data: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    Blake2b::blake2b(&mut out, data, &[]);
    out
}
