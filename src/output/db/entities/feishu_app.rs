use sea_orm::entity::prelude::*;

#[derive(Clone, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "feishu_app")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i64,
    pub name: String,
    pub app_id: String,
    pub app_secret: String,
    pub tenant_key: String,
    pub enabled: bool,
    pub revision: i64,
}
impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FeishuApp")
            .field("id", &self.id)
            .field("app_id", &self.app_id)
            .field("app_secret", &"[REDACTED]")
            .finish()
    }
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
