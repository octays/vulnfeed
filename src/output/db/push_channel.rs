use crate::{
    AppResult,
    domain::models::push_channel::{CreatePushChannelRequest, PushChannel},
    errors::Error,
    output::db::base::{Dao, dao_create, dao_update},
};
use sqlx::{Sqlite, Transaction};

pub struct PushChannelDao;
impl Dao for PushChannelDao {
    const TABLE: &'static str = "push_channel";
}
impl PushChannelDao {
    pub async fn list_enabled(tx: &mut Transaction<'_, Sqlite>) -> AppResult<Vec<PushChannel>> {
        sqlx::query_as::<_, PushChannel>(
            "SELECT * FROM push_channel WHERE status = TRUE ORDER BY id",
        )
        .fetch_all(&mut **tx)
        .await
        .map_err(|e| {
            error_stack::Report::new(Error::Message(format!(
                "list enabled push channels failed: {e}"
            )))
        })
    }

    pub async fn list_all(tx: &mut Transaction<'_, Sqlite>) -> AppResult<Vec<PushChannel>> {
        sqlx::query_as::<_, PushChannel>("SELECT * FROM push_channel ORDER BY id")
            .fetch_all(&mut **tx)
            .await
            .map_err(|e| {
                error_stack::Report::new(Error::Message(format!("list push channels failed: {e}")))
            })
    }

    pub async fn create(
        tx: &mut Transaction<'_, Sqlite>,
        req: CreatePushChannelRequest,
    ) -> AppResult<i64> {
        dao_create::<Self, _>(tx, req).await
    }

    pub async fn update(
        tx: &mut Transaction<'_, Sqlite>,
        id: i64,
        req: CreatePushChannelRequest,
    ) -> AppResult<()> {
        dao_update::<Self, _>(tx, id, req).await?;
        Ok(())
    }

    pub async fn delete(tx: &mut Transaction<'_, Sqlite>, id: i64) -> AppResult<()> {
        sqlx::query("DELETE FROM push_channel WHERE id = ?")
            .bind(id)
            .execute(&mut **tx)
            .await
            .map_err(|e| {
                error_stack::Report::new(Error::Message(format!("delete push channel failed: {e}")))
            })?;
        Ok(())
    }
}
