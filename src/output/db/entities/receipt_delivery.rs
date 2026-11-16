use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "receipt_delivery")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub case_id: i64,
    pub task_id: i64,
    pub app_id: String,
    pub tenant_key: String,
    pub chat_id: String,
    pub message_id: Option<String>,
    pub request_id: String,
    pub task_version: i64,
    pub withdrawn: bool,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
