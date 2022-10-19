use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "AssetTransfer")]
pub struct Model {
    #[sea_orm(primary_key, column_type = "BigInteger")]
    pub id: i64,
    pub utxo_id: i64,
    pub asset_id: Option<i64>, // NULL means ADA
    pub amount: u64,
}

#[derive(Copy, Clone, Debug, DeriveRelation, EnumIter)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::transaction_output::Entity",
        from = "Column::UtxoId",
        to = "super::transaction_output::Column::Id"
    )]
    TransactionOutput,
    #[sea_orm(
        belongs_to = "super::native_asset::Entity",
        from = "Column::AssetId",
        to = "super::native_asset::Column::Id"
    )]
    NativeAsset,
}

// TODO: figure out why this isn't automatically handle by the macros above
impl Related<super::transaction_output::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TransactionOutput.def()
    }
}

// TODO: figure out why this isn't automatically handle by the macros above
impl Related<super::native_asset::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::NativeAsset.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
