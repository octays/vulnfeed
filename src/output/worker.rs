use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use std::sync::Arc;

use error_stack::ResultExt;
use mea::mpsc::UnboundedReceiver;

use crate::{
    AppResult,
    domain::models::vuln_information::CreateVulnInformation,
    errors::Error,
    output::db::{sqlite::Db, vuln_information::VulnInformationDao},
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
            let disclosure = req.disclosure.clone();
            match self.store(req).await {
                Err(e) => {
                    log::error!("Failed to store vuln information: {:?}", e);
                    continue;
                }
                Ok((id, as_new_vuln, inserted)) => {
                    if inserted {
                        match crate::output::receipts::auto_create(
                            &self.db.orm(),
                            "vuln",
                            id,
                            &disclosure,
                        )
                        .await
                        {
                            Ok(Some(case_id)) => {
                                log::info!("Queued automatic receipt case {case_id}")
                            }
                            Ok(None) => {}
                            Err(e) => log::error!(
                                "Failed to automatically create receipt for vulnerability {id}: {e}"
                            ),
                        }
                    }
                    if as_new_vuln
                        && let Err(e) =
                            crate::output::db::vuln_information::VulnInformationDao::mark_pushed(
                                &self.db.orm(),
                                id,
                            )
                            .await
                    {
                        log::error!("Failed to push vuln information {id}: {e:?}");
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn store(&self, mut req: CreateVulnInformation) -> AppResult<(i64, bool, bool)> {
        let db = self.db.orm();
        // Network calls must not keep a SQLite snapshot open. This preliminary
        // lookup only avoids unnecessary searches; the authoritative check below
        // runs again after acquiring the write reservation.
        let lookup = || {
            Statement::from_sql_and_values(
                DatabaseBackend::Sqlite,
                "SELECT EXISTS(SELECT 1 FROM vuln_information WHERE key=? OR (cve<>'' AND cve=?)) AS found",
                [req.key.clone().into(), req.cve.clone().into()],
            )
        };
        let exists: bool = db
            .query_one(lookup())
            .await
            .change_context_lazy(|| {
                Error::Message("failed to check existing vulnerability".into())
            })?
            .ok_or_else(|| Error::Message("missing existence result".into()))?
            .try_get("", "found")
            .change_context_lazy(|| Error::Message("failed to decode existence result".into()))?;
        if !exists && !req.cve.is_empty() {
            req.github_search = search_github_poc(&req.cve).await.into();
        }
        let mut tx = crate::output::db::sqlite::begin_write(&db)
            .await
            .change_context_lazy(|| Error::Message("failed to begin write transaction".into()))?;
        let exists: bool = tx
            .query_one(lookup())
            .await
            .change_context_lazy(|| {
                Error::Message("failed to check existing vulnerability".into())
            })?
            .ok_or_else(|| Error::Message("missing existence result".into()))?
            .try_get("", "found")
            .change_context_lazy(|| Error::Message("failed to decode existence result".into()))?;
        let (id, as_new_vuln) = VulnInformationDao::create_or_update(&mut tx, req).await?;
        tx.commit()
            .await
            .change_context_lazy(|| Error::Message("failed to commit transaction".into()))?;
        if as_new_vuln
            && let Ok(dict) = crate::output::stacks::load_dict(&db).await
            && !dict.is_empty()
        {
            let llm = crate::output::llm_setting::client(&db).await;
            let llm = llm.as_ref();
            let _ = crate::output::stacks::tag_item(&db, "vuln", id, &dict, llm).await;
        }
        Ok((id, as_new_vuln, !exists))
    }
}

#[cfg(test)]
mod tests {
    use super::Worker;
    use crate::{
        config::settings::Config, domain::models::vuln_information::CreateVulnInformation,
        output::db::sqlite::Db,
    };

    #[tokio::test]
    async fn concurrent_stores_of_same_key_create_one_record() {
        let path =
            std::env::temp_dir().join(format!("vulnfeed-worker-{}.db", uuid::Uuid::new_v4()));
        let mut config = Config::default();
        config.database.path = path.to_string_lossy().into_owned();
        let db = Db::new(&config).await.unwrap();
        db.migrate().await.unwrap();
        let (_, receiver1) = mea::mpsc::unbounded();
        let (_, receiver2) = mea::mpsc::unbounded();
        let worker1 = Worker::new(receiver1, db.clone(), false);
        let worker2 = Worker::new(receiver2, db.clone(), false);
        let req = CreateVulnInformation {
            key: "same-key".into(),
            title: "test".into(),
            description: String::new(),
            severity: String::new(),
            cve: String::new(),
            disclosure: String::new(),
            solutions: String::new(),
            reference_links: Vec::new().into(),
            tags: Vec::new().into(),
            github_search: Vec::new().into(),
            source: String::new(),
            source_name: String::new(),
            reasons: Vec::new().into(),
            pushed: false,
            detail_link: String::new(),
        };
        let (a, b) = tokio::join!(worker1.store(req.clone()), worker2.store(req));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.0, b.0);
        assert_ne!(a.2, b.2);
        drop(worker1);
        drop(worker2);
        drop(db);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
