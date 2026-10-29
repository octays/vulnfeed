use std::sync::Arc;

use error_stack::ResultExt;
use mea::mpsc::UnboundedReceiver;

use crate::{
    AppResult,
    domain::models::vuln_information::CreateVulnInformation,
    errors::Error,
    output::{
        db::{push_channel::PushChannelDao, sqlite::Db, vuln_information::VulnInformationDao},
        push::{push_markdown_all, render_vulninfo},
    },
    utils::search::search_github_poc,
};

pub struct Worker {
    pub receiver: UnboundedReceiver<CreateVulnInformation>,
    pub db: Arc<Db>,
    pub push_backlog: bool,
}

impl Worker {
    pub fn new(
        receiver: UnboundedReceiver<CreateVulnInformation>,
        db: Db,
        push_backlog: bool,
    ) -> Self {
        Worker {
            receiver,
            db: Arc::new(db),
            push_backlog,
        }
    }

    pub async fn run(&mut self) -> AppResult<()> {
        while let Some(req) = self.receiver.recv().await {
            match self.store(req).await {
                Err(e) => {
                    log::error!("Failed to store vuln information: {:?}", e);
                    continue;
                }
                Ok((id, as_new_vuln)) => {
                    if as_new_vuln || self.push_backlog {
                        if let Err(e) = self.push_channels(id).await {
                            log::error!("Failed to push vuln information {id}: {e:?}");
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
        let vuln = VulnInformationDao::fetch_by_id(&mut tx, id).await?;
        if let Some(v) = vuln
            && !v.pushed
            && !channels.is_empty()
        {
            let title = v.title.clone();
            let msg = match render_vulninfo(v) {
                Ok(msg) => msg,
                Err(e) => {
                    log::error!("Failed to read vuln info: {:?}", e);
                    return Err(Error::Message(format!("Failed to read vuln info: {:?}", e)).into());
                }
            };
            let ok = push_markdown_all(&channels, title, msg).await?;
            if ok > 0 {
                log::info!("push channels success! id: {} ({} channel(s))", id, ok);
                VulnInformationDao::update_pushed(&mut tx, id, true).await?;
            } else {
                log::warn!("all push channels failed! id: {id}");
            }
        }
        tx.commit()
            .await
            .change_context_lazy(|| Error::Message("failed to commit transaction".to_string()))?;
        Ok(())
    }
    pub async fn store(&self, mut req: CreateVulnInformation) -> AppResult<(i64, bool)> {
        let mut tx =
            self.db.pool.begin().await.change_context_lazy(|| {
                Error::Message("failed to begin transaction".to_string())
            })?;
        if VulnInformationDao::fetch_by_key(&mut tx, &req.key)
            .await?
            .is_none()
            && !req.cve.is_empty()
        {
            req.github_search = search_github_poc(&req.cve).await.into();
        }
        let (id, as_new_vuln) = VulnInformationDao::create_or_update(&mut tx, req).await?;
        tx.commit()
            .await
            .change_context_lazy(|| Error::Message("failed to commit transaction".to_string()))?;
        Ok((id, as_new_vuln))
    }
}
