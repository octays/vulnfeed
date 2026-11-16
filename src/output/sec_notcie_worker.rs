use std::sync::Arc;

use error_stack::ResultExt;
use mea::mpsc::UnboundedReceiver;

use crate::{
    AppResult,
    domain::models::security_notice::CreateSecurityNotice,
    errors::Error,
    output::db::{security_notice::SecurityNoticeDao, sqlite::Db},
};

pub struct SecNoticeWorker {
    pub receiver: UnboundedReceiver<CreateSecurityNotice>,
    pub db: Arc<Db>,
    pub push_backlog: bool,
}

impl SecNoticeWorker {
    pub fn new(
        receiver: UnboundedReceiver<CreateSecurityNotice>,
        db: Db,
        push_backlog: bool,
    ) -> Self {
        SecNoticeWorker {
            receiver,
            db: Arc::new(db),
            push_backlog,
        }
    }

    pub async fn run(&mut self) -> AppResult<()> {
        while let Some(req) = self.receiver.recv().await {
            let publish_time = req.publish_time.clone();
            match self.store(req).await {
                Err(e) => {
                    log::error!("Failed to store security notice: {:?}", e);
                    continue;
                }
                Ok((id, created)) => {
                    if created {
                        match crate::output::receipts::auto_create(
                            &self.db.orm(),
                            "notice",
                            id,
                            &publish_time,
                        )
                        .await
                        {
                            Ok(Some(case_id)) => {
                                log::info!("Queued automatic receipt case {case_id}")
                            }
                            Ok(None) => {}
                            Err(e) => log::error!(
                                "Failed to automatically create receipt for notice {id}: {e}"
                            ),
                        }
                    }
                    if created
                        && let Err(e) =
                            crate::output::db::security_notice::SecurityNoticeDao::mark_pushed(
                                &self.db.orm(),
                                id,
                            )
                            .await
                    {
                        log::error!("Failed to push security notice {id}: {e:?}");
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn store(&self, req: CreateSecurityNotice) -> AppResult<(i64, bool)> {
        let mut tx = crate::output::db::sqlite::begin_write(&self.db.orm())
            .await
            .change_context_lazy(|| Error::Message("failed to begin transaction".to_string()))?;
        let res = SecurityNoticeDao::fetch_by_key(&mut tx, &req.key).await?;
        let (id, created) = match res {
            None => {
                let id = SecurityNoticeDao::create(&mut tx, req).await?;
                (id, true)
            }
            Some(sec_notice) => (sec_notice.id, false),
        };

        tx.commit()
            .await
            .change_context_lazy(|| Error::Message("failed to commit transaction".to_string()))?;
        if created
            && let Ok(dict) = crate::output::stacks::load_dict(&self.db.orm()).await
            && !dict.is_empty()
        {
            let llm = crate::output::llm_setting::client(&self.db.orm()).await;
            let llm = llm.as_ref();
            let _ = crate::output::stacks::tag_item(&self.db.orm(), "notice", id, &dict, llm).await;
        }
        Ok((id, created))
    }
}
