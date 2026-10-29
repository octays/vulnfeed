use std::sync::Arc;

use error_stack::ResultExt;
use mea::mpsc::UnboundedReceiver;

use crate::{
    AppResult,
    domain::models::security_notice::CreateSecurityNotice,
    errors::Error,
    output::{
        db::{push_channel::PushChannelDao, security_notice::SecurityNoticeDao, sqlite::Db},
        push::{push_markdown_all, render_sec_notice},
    },
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
            match self.store(req).await {
                Err(e) => {
                    log::error!("Failed to store security notice: {:?}", e);
                    continue;
                }
                Ok((id, created)) => {
                    if created || self.push_backlog {
                        if let Err(e) = self.push_channels(id).await {
                            log::error!("Failed to push security notice {id}: {e:?}");
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn push_channels(&self, id: i64) -> AppResult<()> {
        log::info!("push channels start! id: {}", id);
        let mut tx =
            self.db.pool.begin().await.change_context_lazy(|| {
                Error::Message("failed to begin transaction".to_string())
            })?;
        let channels = PushChannelDao::list_enabled(&mut tx).await?;
        let sec_notice = SecurityNoticeDao::fetch_by_id(&mut tx, id).await?;
        if let Some(n) = sec_notice
            && !n.pushed
            && !channels.is_empty()
        {
            let title = n.title.clone();
            let msg = match render_sec_notice(n) {
                Ok(msg) => msg,
                Err(e) => {
                    log::error!("Failed to read sec notice: {:?}", e);
                    return Err(
                        Error::Message(format!("Failed to read sec notice: {:?}", e)).into(),
                    );
                }
            };
            let ok = push_markdown_all(&channels, title, msg).await?;
            if ok > 0 {
                log::info!("push channels success! id: {} ({} channel(s))", id, ok);
                SecurityNoticeDao::update_pushed(&mut tx, id, true).await?;
            } else {
                log::warn!("all push channels failed! id: {id}");
            }
        }
        tx.commit()
            .await
            .change_context_lazy(|| Error::Message("failed to commit transaction".to_string()))?;
        Ok(())
    }
    pub async fn store(&self, req: CreateSecurityNotice) -> AppResult<(i64, bool)> {
        let mut tx =
            self.db.pool.begin().await.change_context_lazy(|| {
                Error::Message("failed to begin transaction".to_string())
            })?;
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
        Ok((id, created))
    }
}
