use std::collections::BTreeSet;

use entity::{
    prelude::*,
    sea_orm::{
        entity::*, prelude::*, ColumnTrait, Condition, DatabaseTransaction, QueryOrder, Set,
    },
};
use std::collections::BTreeMap;

static ADDRESS_TRUNCATE: usize = 500; // 1000 in hex

pub fn get_truncated_address(addr_bytes: &[u8]) -> &[u8] {
    &addr_bytes[0..(std::cmp::min(addr_bytes.len(), ADDRESS_TRUNCATE))]
}

pub struct AddressInBlock {
    pub model: AddressModel,
    pub is_new: bool,
}

pub async fn insert_addresses(
    addresses: &BTreeMap<Vec<u8>, i64>,
    txn: &DatabaseTransaction,
) -> Result<BTreeMap<Vec<u8>, AddressInBlock>, DbErr> {
    if addresses.is_empty() {
        return Ok(BTreeMap::default());
    }
    // During the Byron era of Cardano,
    // Addresses had a feature where you could add extra metadata in them
    // The amount of metadata you could insert was not capped
    // So some addresses got generated which are really large
    // However, Postgres btree v4 has a maximum size of 2704 for an index
    // Since these addresses can't be spent anyway, we just truncate them
    // theoretically, we could truncate at 2704, but we truncate at ADDRESS_TRUNCATE
    // reasons:
    // 1) Postgres has shrunk the limit in the past, so they may do it again
    // 2) Use of the INCLUDE in creating an index can increase its size
    //    So best to leave some extra room incase this is useful someday
    // 3) It's not great to hard-code a postgresql-specific limitation
    // 4) ADDRESS_TRUNCATE seems more obviously human than 2704 so maybe easier if somebody sees it
    // 5) Storing up to 2704 bytes is a waste of space since they aren't used for anything
    let truncated_addrs: BTreeMap<&[u8], i64> = addresses
        .iter()
        .map(|addr| (get_truncated_address(addr.0.as_slice()), *addr.1))
        .collect();

    // deduplicate addresses to avoid re-querying the same address many times
    // useful not only as a perf improvement, but also avoids parallel queries writing to the same row
    let deduplicated = BTreeSet::<_>::from_iter(truncated_addrs.keys().copied());

    let mut result_map = BTreeMap::<Vec<u8>, AddressInBlock>::default();

    // 1) Add addresses that were already in the DB
    {
        // note: in the usual case, the address will already be in the DB when we query it
        // that means it's faster to use find instead of write(on conflict do nothing)
        // since "do nothing" returns None, a conflict mean we would have to use find as a fallback
        // meaning the "on conflict do nothing" requires 2 queries in the usual case instead of 1

        // note: okay to batch use "all" since we're querying unique keys
        let mut found_addresses = Address::find()
            .filter(Condition::any().add(AddressColumn::Payload.is_in(deduplicated.clone())))
            .all(txn)
            .await?;

        // add addresses that already existed previously directly to the result
        result_map.extend(found_addresses.drain(..).map(|model| {
            (
                model.payload.clone(),
                AddressInBlock {
                    model,
                    is_new: false,
                },
            )
        }));
    }

    // 2) Add addresses that weren't in the DB
    {
        // check which addresses weren't found in the DB and prepare to add them
        let mut addrs_to_add: Vec<AddressActiveModel> = deduplicated
            .iter()
            .filter(|&&addr| !result_map.contains_key(addr))
            .map(|addr| AddressActiveModel {
                payload: Set(addr.to_vec()),
                first_tx: Set(truncated_addrs[addr]),
                ..Default::default()
            })
            .collect();

        // need to make sure we're inserting addresses in the same order as we added txs
        addrs_to_add.sort_by(|a, b| a.first_tx.as_ref().cmp(b.first_tx.as_ref()));

        // add the new entires into the DB, then add them to our result mapping
        if !addrs_to_add.is_empty() {
            Address::insert_many(addrs_to_add)
                .exec_many_with_returning(txn)
                .await?
                .iter()
                .for_each(|model| {
                    result_map.insert(
                        model.payload.clone(),
                        AddressInBlock {
                            model: model.clone(),
                            is_new: true,
                        },
                    );
                });
        }
    }

    Ok(result_map)
}

pub async fn get_outputs_for_inputs(
    inputs: &[(Vec<pallas::ledger::traverse::OutputRef>, i64)],
    txn: &DatabaseTransaction,
) -> Result<Vec<(TransactionOutputModel, TransactionModel)>, DbErr> {
    // avoid querying the DB if there were no inputs
    let has_input = inputs.iter().any(|input| !input.0.is_empty());
    if !has_input {
        return Ok(vec![]);
    }

    // 1) Get the UTXO this input is spending
    let mut output_conditions = Condition::any();

    // note: we don't need to deduplicate the conditions because every UTXO can only be spent once
    // so we know all these pairs are disjoint amongst all transactions
    // https://github.com/dcSpark/carp/issues/46
    for input in inputs.iter().flat_map(|inputs| inputs.0.iter()) {
        output_conditions = output_conditions.add(
            Condition::all()
                .add(TransactionOutputColumn::OutputIndex.eq(input.index()))
                .add(TransactionColumn::Hash.eq(input.hash().to_vec())),
        );
    }

    let mut tx_outputs = TransactionOutput::find()
        .inner_join(Transaction)
        .filter(output_conditions)
        .select_with(Transaction)
        // TODO: we only actually need these columns, but sea-orm returns the full join
        // .column(TransactionOutputColumn::Id)
        // .column(TransactionOutputColumn::OutputIndex)
        // .column(TransactionOutputColumn::Payload)
        // .column(TransactionColumn::Hash)
        // .column(TransactionColumn::Id)
        // note: we can use "all" because all utxos are unique so we know:
        // 1) there won't be duplicates in the result set
        // 2) the # results == # of outputs in the filter
        .all(txn)
        .await?;

    Ok(tx_outputs
        .drain(..)
        // <tx, tx_out> is a one-to-one mapping so it's safe to flatten this
        .map(|(output, txs)| {
            if txs.len() > 1 {
                panic!();
            }
            (output, txs[0].clone())
        })
        .collect())
}

pub fn gen_input_to_output_map<'a>(
    outputs_for_inputs: &'a [(TransactionOutputModel, TransactionModel)],
) -> BTreeMap<&'a Vec<u8>, BTreeMap<i64, &'a TransactionOutputModel>> {
    let mut input_to_output_map =
        BTreeMap::<&Vec<u8>, BTreeMap<i64, &'a TransactionOutputModel>>::default();
    for output in outputs_for_inputs {
        input_to_output_map
            .entry(&output.1.hash)
            .and_modify(|output_index_map| {
                // note: we can insert right away instead of doing a 2nd lookup
                // because the pair <payload, output_index> is unique
                output_index_map.insert(output.0.output_index as i64, &output.0);
            })
            .or_insert({
                let mut output_index_map = BTreeMap::<i64, &'a TransactionOutputModel>::default();
                output_index_map.insert(output.0.output_index as i64, &output.0);
                output_index_map
            });
    }

    input_to_output_map
}

pub async fn insert_inputs(
    inputs: &[(Vec<pallas::ledger::traverse::OutputRef>, i64)],
    input_to_output_map: &BTreeMap<&Vec<u8>, BTreeMap<i64, &TransactionOutputModel>>,
    txn: &DatabaseTransaction,
) -> Result<Vec<TransactionInputModel>, DbErr> {
    // avoid querying the DB if there were no inputs
    let has_input = inputs.iter().any(|input| !input.0.is_empty());
    if !has_input {
        return Ok(vec![]);
    }

    let result = TransactionInput::insert_many(
        inputs
            .iter()
            .flat_map(|pair| pair.0.iter().enumerate().zip(std::iter::repeat(pair.1)))
            .map(|((idx, input), tx_id)| {
                let tx_outputs = match input_to_output_map.get(&input.hash().to_vec()) {
                    Some(outputs) => outputs,
                    None => panic!("Failed to find transaction {}", &hex::encode(input.hash())),
                };
                let output = tx_outputs[&(input.index() as i64)];
                TransactionInputActiveModel {
                    utxo_id: Set(output.id),
                    address_id: Set(output.address_id),
                    tx_id: Set(tx_id),
                    input_index: Set(idx as i32),
                    ..Default::default()
                }
            }),
    )
    .exec_many_with_returning(txn)
    .await?;

    Ok(result)
}

pub async fn transactions_from_hashes(
    db_tx: &DatabaseTransaction,
    tx_hashes: &[Vec<u8>],
) -> Result<Vec<TransactionModel>, DbErr> {
    use entity::sea_orm::QueryOrder;
    let txs = Transaction::find()
        .filter(TransactionColumn::Hash.is_in(tx_hashes.to_vec()))
        .order_by_asc(TransactionColumn::Id)
        .all(db_tx)
        .await?;
    if txs.len() != tx_hashes.len() {
        let mut remaining = BTreeSet::<_>::from_iter(tx_hashes.iter());
        for tx in &txs {
            remaining.remove(&tx.hash);
        }
        if !remaining.is_empty() {
            panic!(
                "Transaction(s) not found in database: {:?}",
                remaining.iter().map(hex::encode)
            );
        }
    }
    Ok(txs)
}

pub async fn block_from_hash(
    db_tx: &DatabaseTransaction,
    hash: &[u8],
) -> Result<BlockModel, DbErr> {
    let block = Block::find()
        .filter(BlockColumn::Hash.eq(hash.to_vec()))
        .one(db_tx)
        .await?;
    Ok(match block {
        None => {
            panic!("Block not found in database: {}", hex::encode(hash));
        }
        Some(block) => block,
    })
}

pub async fn output_from_pointer(
    db_tx: &DatabaseTransaction,
    pointers: &[(i64 /* txid */, usize /* output index */)],
) -> Result<Vec<TransactionOutputModel>, DbErr> {
    // https://github.com/dcSpark/carp/issues/46
    let mut output_conditions = Condition::any();
    for (tx_id, output_index) in pointers.iter() {
        output_conditions = output_conditions.add(
            Condition::all()
                .add(TransactionOutputColumn::TxId.eq(*tx_id))
                .add(TransactionOutputColumn::OutputIndex.eq(*output_index as i32)),
        );
    }

    let outputs = TransactionOutput::find()
        .filter(output_conditions)
        .order_by_asc(TransactionOutputColumn::Id)
        .all(db_tx)
        .await?;
    Ok(outputs)
}
pub async fn input_from_pointer(
    db_tx: &DatabaseTransaction,
    pointers: &[(i64 /* txid */, usize /* input index */)],
) -> Result<Vec<TransactionInputModel>, DbErr> {
    // https://github.com/dcSpark/carp/issues/46
    let mut input_conditions = Condition::any();
    for (tx_id, input_index) in pointers.iter() {
        input_conditions = input_conditions.add(
            Condition::all()
                .add(TransactionInputColumn::TxId.eq(*tx_id))
                .add(TransactionInputColumn::InputIndex.eq(*input_index as i32)),
        );
    }

    let inputs = TransactionInput::find()
        .filter(input_conditions)
        .order_by_asc(TransactionInputColumn::Id)
        .all(db_tx)
        .await?;
    Ok(inputs)
}
