use std::collections::{BTreeMap, BTreeSet};

use super::multiera_asset_mint::MultieraAssetMintTask;
use super::multiera_used_outputs::MultieraOutputTask;
use super::utils::common::asset_from_pair;
use crate::config::EmptyConfig::EmptyConfig;
use entity::asset_transfer;
use entity::{
    prelude::*,
    sea_orm::{prelude::*, DatabaseTransaction, Set},
};
use pallas::ledger::traverse::{Asset, Subject};

use crate::dsl::task_macro::*;

carp_task! {
  name MultieraAssetTransferTask;
  configuration EmptyConfig;
  doc "Adds assets and their amounts from each used output (regular outputs in most cases, collateral return if tx fails)";
  era multiera;
  dependencies [MultieraOutputTask, MultieraAssetMintTask];
  read [multiera_queued_asset_transfers, multiera_outputs, multiera_assets];
  write [];
  should_add_task |block, _properties| {
    block.1.txs().iter().any(|tx| if tx.is_valid() {
      tx.outputs().len() > 0
    } else {
      tx.collateral_return().is_some()
    })
  };
  execute |previous_data, task| handle_assets(
      task.db_tx,
      &previous_data.multiera_queued_asset_transfers,
      &previous_data.multiera_outputs,
      &previous_data.multiera_assets,
  );
  merge_result |previous_data, _result| {
  };
}

pub struct QueuedAssetTransfer {
    pub output_pointer: (i64, usize),
    pub asset: Asset,
}

async fn handle_assets(
    db_tx: &DatabaseTransaction,
    multiera_queued_asset_transfers: &[QueuedAssetTransfer],
    multiera_outputs: &[TransactionOutputModel],
    multiera_assets: &[NativeAssetModel],
) -> Result<(), DbErr> {
    // 1. Prepare output ids
    let mut output_pointer_to_id_map = BTreeMap::<(i64, usize), i64>::default();
    for output in multiera_outputs {
        output_pointer_to_id_map.insert((output.tx_id, output.output_index as usize), output.id);
    }

    // 2. Query for asset ids in multiera_assets and the database
    let mut asset_pair_to_id_map = BTreeMap::<(Vec<u8>, Vec<u8>), i64>::default();
    for asset in multiera_assets {
        asset_pair_to_id_map.insert(
            (asset.policy_id.clone(), asset.asset_name.clone()),
            asset.id,
        );
    }

    let mut pairs_to_query = BTreeSet::<(Vec<u8>, Vec<u8>)>::default();
    for asset_transfer in multiera_queued_asset_transfers {
        if let Subject::NativeAsset(policy_id, asset_name) = &asset_transfer.asset.subject {
            let pair = (policy_id.to_vec(), asset_name.to_vec());
            if !asset_pair_to_id_map.contains_key(&pair) {
                pairs_to_query.insert(pair);
            }
        }
    }
    let found_assets =
        asset_from_pair(db_tx, &pairs_to_query.into_iter().collect::<Vec<_>>()).await?;
    for asset in found_assets {
        asset_pair_to_id_map.insert((asset.policy_id, asset.asset_name), asset.id);
    }

    // 3. Insert asset transfers
    let to_insert = multiera_queued_asset_transfers
        .iter()
        .map(|asset_transfer| AssetTransferActiveModel {
            utxo_id: Set(output_pointer_to_id_map[&asset_transfer.output_pointer]),
            asset_id: Set(match &asset_transfer.asset.subject {
                Subject::Lovelace => None,
                Subject::NativeAsset(policy_id, asset_name) => {
                    Some(asset_pair_to_id_map[&(policy_id.to_vec(), asset_name.to_vec())])
                }
            }),
            amount: Set(asset_transfer.asset.quantity),
            ..Default::default()
        });
    AssetTransfer::insert_many(to_insert).exec(db_tx).await?;

    Ok(())
}
