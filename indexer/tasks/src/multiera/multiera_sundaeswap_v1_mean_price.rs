use super::dex::common::{handle_mean_price, PoolType};
use super::multiera_address::MultieraAddressTask;
use crate::config::EmptyConfig::EmptyConfig;
use crate::dsl::task_macro::*;

carp_task! {
  name MultieraSundaeSwapV1MeanPriceTask;
  configuration EmptyConfig;
  doc "Adds SundaeSwap V1 mean price updates to the database";
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
      PoolType::SundaeSwapV1,
  );
  merge_result |previous_data, _result| {
  };
}
