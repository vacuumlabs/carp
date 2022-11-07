use std::collections::BTreeSet;

use super::utils::common::{
    get_asset_amount, get_plutus_datum_for_output, get_sheley_payment_hash,
};
use super::{multiera_address::MultieraAddressTask, utils::common::asset_from_pair};
use crate::dsl::task_macro::*;
use crate::{config::EmptyConfig::EmptyConfig, types::AssetPair};
use entity::sea_orm::{DatabaseTransaction, Set};
use pallas::ledger::{
    primitives::ToCanonicalJson,
    traverse::{MultiEraBlock, MultiEraTx},
};

const POOL_SCRIPT_HASH: &str = "e1317b152faac13426e6a83e06ff88a4d62cce3c1634ab0a5ec13309";

carp_task! {
    name MultieraMinswapV1MeanPriceTask;
    configuration EmptyConfig;
    doc "Adds Minswap V1 mean price updates to the database";
    era multiera;
    dependencies [MultieraAddressTask];
    read [multiera_txs, multiera_addresses];
    write [];
    should_add_task |block, _properties| {
      block.1.txs().iter().any(|tx| tx.outputs().len() > 0)
    };
    execute |previous_data, task| handle_mean_price(
        task.db_tx,
        task.block,
        &previous_data.multiera_txs,
        &previous_data.multiera_addresses,
    );
    merge_result |previous_data, _result| {
    };
}


struct QueuedMeanPrice {
    tx_id: i64,
    address: Vec<u8>, // pallas::crypto::hash::Hash<32>
    asset1: AssetPair,
    asset2: AssetPair,
    amount1: u64,
    amount2: u64,
}


async fn handle_mean_price(
    db_tx: &DatabaseTransaction,
    block: BlockInfo<'_, MultiEraBlock<'_>>,
    multiera_txs: &[TransactionModel],
    multiera_addresses: &BTreeMap<Vec<u8>, AddressInBlock>,
) -> Result<(), DbErr> {
    // 1) Parse mean prices
    let mut queued_prices = Vec::<QueuedMeanPrice>::default();
    for (tx_body, cardano_transaction) in block.1.txs().iter().zip(multiera_txs) {
        if cardano_transaction.is_valid {
            queue_mean_price(&mut queued_prices, tx_body, cardano_transaction.id);
        }
    }

    if queued_prices.is_empty() {
        return Ok(());
    }


    // 2) Remove asset duplicates to build a list of all the <policy_id, asset_name> to query for.
    // ADA is ignored, it's not in the NativeAsset DB table
    let mut unique_tokens = BTreeSet::<&(Vec<u8>, Vec<u8>)>::default();
    for p in &queued_prices {
        if let Some(pair) = &p.asset1 {
            unique_tokens.insert(&pair);
        }
        if let Some(pair) = &p.asset2 {
            unique_tokens.insert(&pair);
        }
    }

    // 3) Query for asset ids
    let found_assets = asset_from_pair(
        db_tx,
        &unique_tokens
            .iter()
            .map(|(policy_id, asset_name)| (policy_id.clone(), asset_name.clone()))
            .collect::<Vec<_>>(),
    )
    .await?;
    let mut asset_pair_to_id_map = found_assets
        .into_iter()
        .map(|asset| (Some((asset.policy_id, asset.asset_name)), Some(asset.id)))
        .collect::<BTreeMap<_, _>>();
    asset_pair_to_id_map.insert(None, None); // ADA

    // 4) Add mean prices to DB
    DexMeanPrice::insert_many(queued_prices.iter().map(|price| DexMeanPriceActiveModel {
        tx_id: Set(price.tx_id),
        address_id: Set(multiera_addresses[&price.address].model.id),
        asset1_id: Set(asset_pair_to_id_map[&price.asset1]),
        asset2_id: Set(asset_pair_to_id_map[&price.asset2]),
        amount1: Set(price.amount1),
        amount2: Set(price.amount2),
        ..Default::default()
    }))
    .exec(db_tx)
    .await?;

    Ok(())
}




fn queue_mean_price(queued_prices: &mut Vec<QueuedMeanPrice>, tx: &MultiEraTx, tx_id: i64) {
    // Find the pool address (Note: there should be at most one pool output)
    for output in tx
        .outputs()
        .iter()
        .find(|o| get_sheley_payment_hash(o.address()).as_deref() == Some(POOL_SCRIPT_HASH))
    {
        // Remark: The datum that corresponds to the pool output's datum hash should be present
        // in tx.plutus_data()
        if let Some(datum) = get_plutus_datum_for_output(output, &tx.plutus_data()) {
            let datum = datum.to_json();
            
            let get_asset_item = |i, j| {
                let item = datum["fields"][1]["fields"][0]["fields"][i]["fields"][j]["bytes"]
                    .as_str()
                    .unwrap()
                    .to_string();
                hex::decode(item).unwrap()
            };
            let get_asset = |policy_id: Vec<u8>, asset_name: Vec<u8>| {
                if policy_id.is_empty() && asset_name.is_empty() {
                    None
                } else {
                    Some((policy_id, asset_name))
                }
            };
            // extract plutus
            let asset1 = get_asset(get_asset_item(0, 0), get_asset_item(0, 1));
            let asset2 = get_asset(get_asset_item(1, 0), get_asset_item(1, 1));
            
            let amount1 = get_asset_amount(output, &asset1);
            let amount2 = get_asset_amount(output, &asset2);

            queued_prices.push(QueuedMeanPrice {
                tx_id,
                address: output.address().unwrap().to_vec(),
                asset1,
                asset2,
                amount1,
                amount2,
            });
        }
    }
}
/*
fn extract_plutus(datum: &serde_json::Value) -> (Asset, Asset) {
    (
        Asset {
            name: datum["fields"][0]["fields"][1]["bytes"]
                .as_str()
                .unwrap()
                .to_string(),
            policy_id: datum["fields"][0]["fields"][0]["bytes"]
                .as_str()
                .unwrap()
                .to_string(),
        },
        Asset {
            name: datum["fields"][1]["fields"][1]["bytes"]
                .as_str()
                .unwrap()
                .to_string(),
            policy_id: datum["fields"][1]["fields"][0]["bytes"]
                .as_str()
                .unwrap()
                .to_string(),
        },
    )
}

#[allow(dead_code)]
pub fn get_address_from_plutus(datum: &serde_json::Value) -> String {
    let first = datum["fields"][1]["fields"][0]["fields"][0]["bytes"]
        .as_str()
        .unwrap()
        .to_string();

    let second = datum["fields"][1]["fields"][1]["fields"][0]["fields"][0]["fields"][0]["bytes"]
        .as_str()
        .unwrap()
        .to_string();

    let string_list = vec![String::from("01"), first, second];
    Address::from_hex(&string_list.join(""))
        .unwrap()
        .to_bech32()
        .unwrap()
}


async fn mean_value(
    &self,
    pool: &PoolConfig,
    _db: &DatabaseConnection,
    transaction: &TransactionRecord,
) -> Option<(AssetAmount, AssetAmount)> {
    let script_hash = hex::decode(&pool.script_hash).unwrap();
    if let Some(output) = transaction
        .outputs
        .iter()
        .flatten()
        .find(|&o| utils::get_payment_hash(&o.address) == Some(script_hash.to_vec()))
    {
        //tracing::info!("output: {:?}, {:?}", output, output.datum_hash);
        if let Some(datum) = transaction
            .plutus_data
            .iter()
            .flatten()
            .find(|p| Some(p.datum_hash.clone()) == output.datum_hash)
        {
            let (asset1, asset2) = extract_plutus(&datum.plutus_data);

            tracing::info!(
                "[{}] {}:{} vs {}:{}",
                transaction.hash,
                asset1.policy_id,
                asset1.name,
                asset2.policy_id,
                asset2.name
            );
            let amount1 = common::get_amount(output, &asset1.policy_id, &asset1.name);
            let amount2 = common::get_amount(output, &asset2.policy_id, &asset2.name);
            tracing::info!("{} vs {}", amount1, amount2);
            return Some((
                AssetAmount {
                    asset: asset1,
                    amount: amount1,
                },
                AssetAmount {
                    asset: asset2,
                    amount: amount2,
                },
            ));
        }
    }

    None
}*/