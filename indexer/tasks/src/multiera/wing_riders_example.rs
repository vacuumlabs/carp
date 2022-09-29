use super::multiera_txs::MultieraTransactionTask;
use crate::config::EmptyConfig::EmptyConfig;
use cardano_multiplatform_lib::address;
use entity::sea_orm::DatabaseTransaction;

use crate::dsl::task_macro::*;

carp_task! {
    name WingRidersTask;
    configuration EmptyConfig;
    doc "Look at some Wingriders transactions.";
    era multiera;
    dependencies [MultieraTransactionTask];
    read [multiera_txs];
    write [];
    should_add_task |block, _properties| {
      true
    };
    execute |previous_data, task| handle_wr(
        task.db_tx,
        task.block,
        &previous_data.multiera_txs,
    );
    merge_result |previous_data, _result| {
    };
  }

async fn handle_wr(
    db_tx: &DatabaseTransaction,
    block: BlockInfo<'_, MultiEraBlock<'_>>,
    multiera_txs: &[TransactionModel],
) -> Result<(), DbErr> {
    let txs = block.1.txs();

    for tx in txs {
        let mut is_wr = false;
        for output in tx.outputs() {
            // address of the ADA/DANA pool
            let addr = output.address().unwrap().to_hex();
            if  addr == "11e6c90a5923713af5786963dee0fdffd830ca7e0c86a041d9e5833e918e99e702b4a39bfd5c4e25437f3100e89e4065e6cb5c72bcff8b4a09" {
                is_wr = true;
            }
        }
        
        if is_wr {
            println!("Yay WingRiders!\n");
        }
    };

    Ok(())
}