use sea_schema::migration::prelude::*;

use entity::prelude::{ NativeAsset, NativeAssetColumn, TransactionOutput, TransactionOutputColumn };
use entity::asset_transfer::*;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20221017_000014_create_asset_transfer_table"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(Entity)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Column::Id)
                            .big_integer()
                            .not_null()
                            .auto_increment()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Column::UtxoId).big_integer().not_null())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-asset_transfer-utxo_id")
                            .from(Entity, Column::UtxoId)
                            .to(TransactionOutput, TransactionOutputColumn::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .col(ColumnDef::new(Column::AssetId).big_integer())
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk-asset_transfer-asset_id")
                            .from(Entity, Column::AssetId)
                            .to(NativeAsset, NativeAssetColumn::Id)
                            // TODO: sea-query doesn't support RESTRICT DEFERRED
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .col(ColumnDef::new(Column::Amount).big_unsigned().not_null())
                    .to_owned(),
            )
            .await?;

        // This creates an index on <UtxoId> and <UtxoId, AssetId> (https://stackoverflow.com/a/11352543)
        // so we also need to explicitly create an index on AssetId
        manager
            .create_index(
                Index::create()
                    .table(Entity)
                    .name("index-asset_transfer-transation_output-native_asset")
                    .col(Column::UtxoId)
                    .col(Column::AssetId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .table(Entity)
                    .name("index-asset_transfer-native_asset")
                    .col(Column::AssetId)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Entity).to_owned())
            .await
    }
}
