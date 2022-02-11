use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Deserialize, Serialize)]
#[sea_orm(table_name = "TransactionInput")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub utxo_id: i32,
    pub tx_id: i32,
    pub input_index: i32,
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
        belongs_to = "super::transaction::Entity",
        from = "Column::TxId",
        to = "super::transaction::Column::Id"
    )]
    Transaction,
}

// TODO: figure out why this isn't automatically handle by the macros above
impl Related<super::transaction_output::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::TransactionOutput.def()
    }
}

// TODO: figure out why this isn't automatically handle by the macros above
impl Related<super::transaction::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Transaction.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
