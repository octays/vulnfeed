use std::{path::PathBuf, sync::Arc};

use crate::{
    AppResult,
    config::settings::{Config, LoadConfigResult, load_config},
    domain::{
        models::{
            admin_user::{AdminUserPassword, AdminUsername, CreateAdminUserRequest},
            security_notice::CreateSecurityNotice,
            vuln_information::CreateVulnInformation,
        },
        ports::VulnService,
        services::Service,
    },
    errors::Error,
    input::http::http_server::{self, make_acceptor},
    output::{
        db::{admin_user::AdminUserDao, sqlite::Db},
        plugins::{sec_notice, vuln},
        scheduler::Scheduler,
        sec_notcie_worker::SecNoticeWorker,
        worker::Worker,
    },
    utils::{
        auth::jwt::JWT,
        num_cpus,
        password_hash::compute_password_hash,
        runtime::{Runtime, make_runtime},
        telemetry,
    },
};
use clap::ValueHint;
use error_stack::ResultExt;

#[derive(Clone)]
pub struct Ctx<S: VulnService + Send + Sync + 'static> {
    pub vuln_service: Arc<S>,
    pub config: Arc<Config>,
    pub sched: Arc<Scheduler>,
    pub jwt: Arc<JWT>,
}

#[derive(Debug, clap::Parser)]
pub struct CommandStart {
    #[clap(short, long, help = "Path to config file", value_hint = ValueHint::FilePath)]
    config_file: PathBuf,
}

impl CommandStart {
    pub fn run(self) -> AppResult<()> {
        error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);
        let LoadConfigResult { config, warnings } = load_config(self.config_file)?;
        let telemetry_runtime = make_telemetry_runtime();
        let mut drop_guards =
            telemetry::init(&telemetry_runtime, "vulnfeed", config.telemetry.clone());
        drop_guards.push(Box::new(telemetry_runtime));
        for warning in warnings {
            log::warn!("{warning}");
        }
        log::info!("server is starting with config: {config:#?}");
        let server_runtime = make_vulnfeed_runtime();
        server_runtime
            .block_on(run_server(&server_runtime, config))
            .change_context(Error::Message("failed to start server".to_string()))?;
        Ok(())
    }
}

async fn run_server(server_rt: &Runtime, config: Config) -> AppResult<()> {
    let make_error = || Error::Message("failed to start server".to_string());
    let (shutdown_tx, shutdown_rx) = mea::shutdown::new_pair();
    let acceptor = make_acceptor(&config.server.listen_addr)
        .await
        .change_context_lazy(make_error)?;

    let jwt = JWT::new(&config.auth.jwt.secret);

    let db = Db::new(&config).await.change_context_lazy(make_error)?;

    sqlx::migrate!("./migrations")
        .run(&db.pool)
        .await
        .change_context_lazy(make_error)?;

    seed_default_admin(&db)
        .await
        .change_context_lazy(make_error)?;
    let (sender, receiver) = mea::mpsc::unbounded::<CreateVulnInformation>();
    vuln::init(sender).change_context_lazy(make_error)?;

    let mut worker = Worker::new(receiver, db.clone());
    server_rt.spawn(async move { worker.run().await });

    let (sec_sender, sec_receiver) = mea::mpsc::unbounded::<CreateSecurityNotice>();
    sec_notice::init(sec_sender).change_context_lazy(make_error)?;

    let mut sec_worker = SecNoticeWorker::new(sec_receiver, db.clone());
    server_rt.spawn(async move { sec_worker.run().await });

    let sched = Scheduler::try_new(db.clone())
        .await
        .change_context_lazy(make_error)?;

    let sched = sched.init_from_db().await.change_context_lazy(make_error)?;
    let vuln_service = Service::new(db);
    let ctx = Ctx {
        vuln_service: Arc::new(vuln_service),
        config: Arc::new(config),
        sched: Arc::new(sched),
        jwt: Arc::new(jwt),
    };

    let server = http_server::start_server(ctx, server_rt, shutdown_rx, acceptor)
        .await
        .change_context_lazy(|| {
            Error::Message("A fatal error has occurred in server process.".to_string())
        })?;

    ctrlc::set_handler(move || shutdown_tx.shutdown()).change_context_lazy(|| {
        Error::Message("failed to setup ctrl-c signal handle".to_string())
    })?;

    server.await_shutdown().await;
    Ok(())
}

fn make_vulnfeed_runtime() -> Runtime {
    let parallelism = num_cpus().get();
    make_runtime("vulnfeed_runtime", "vulnfeed_thread", parallelism)
}

fn make_telemetry_runtime() -> Runtime {
    make_runtime("telemetry_runtime", "telemetry_thread", 1)
}

fn make_init_data_runtime() -> Runtime {
    make_runtime("init_data_runtime", "init_data_thread", 1)
}

#[derive(Debug, clap::Parser)]
pub struct ChangeSuperUserPassword {
    #[clap(short, long, help = "Path to config file", value_hint = ValueHint::FilePath)]
    config_file: PathBuf,
    #[clap(short, long, help = "New password for the super user (4-128 chars)")]
    password: String,
}
impl ChangeSuperUserPassword {
    pub fn run(self) -> AppResult<()> {
        error_stack::Report::set_color_mode(error_stack::fmt::ColorMode::None);
        let LoadConfigResult { config, warnings } = load_config(self.config_file)?;
        let telemetry_runtime = make_telemetry_runtime();
        let mut drop_guards =
            telemetry::init(&telemetry_runtime, "init-data", config.telemetry.clone());
        drop_guards.push(Box::new(telemetry_runtime));
        for warning in warnings {
            log::warn!("{warning}");
        }
        let init_data_runtime = make_init_data_runtime();
        init_data_runtime
            .block_on(run_change_super_user_password(config, self.password))
            .change_context_lazy(|| {
                Error::Message("failed to change super user password".to_string())
            })?;
        Ok(())
    }
}
async fn run_change_super_user_password(config: Config, password: String) -> AppResult<()> {
    let make_error = || Error::Message("failed to change super user password".to_string());
    let invalid_password =
        || Error::Message("invalid password: must be 4-128 characters long".to_string());
    // 校验原始密码(而非其哈希),与登录侧同一套规则;不合法则显式报错退出
    let password = AdminUserPassword::try_new(password).change_context_lazy(invalid_password)?;
    let password_hash = compute_password_hash(password.as_ref()).change_context_lazy(make_error)?;
    let db = Db::new(&config).await.change_context_lazy(make_error)?;
    sqlx::migrate!("./migrations")
        .run(&db.pool)
        .await
        .change_context_lazy(make_error)?;
    let mut tx = db.pool.begin().await.change_context_lazy(make_error)?;
    let req = CreateAdminUserRequest::new(
        AdminUsername::try_new("admin").change_context_lazy(make_error)?,
        AdminUserPassword::try_new(password_hash).change_context_lazy(make_error)?,
    );
    AdminUserDao::create_super_user(&mut tx, req).await?;
    tx.commit().await.change_context_lazy(make_error)?;
    log::info!("change super user password success");
    Ok(())
}
/// admin_user 表无 admin 记录时(首次启动)播种默认超管 admin/admin
async fn seed_default_admin(db: &Db) -> AppResult<()> {
    let make_error = || Error::Message("failed to seed default admin user".to_string());
    let mut tx = db.pool.begin().await.change_context_lazy(make_error)?;
    let name = AdminUsername::try_new("admin").change_context_lazy(make_error)?;
    if AdminUserDao::fetch_by_name(&mut tx, &name).await?.is_none() {
        let password_hash = compute_password_hash("admin").change_context_lazy(make_error)?;
        let req = CreateAdminUserRequest::new(
            name.clone(),
            AdminUserPassword::try_new(password_hash).change_context_lazy(make_error)?,
        );
        AdminUserDao::create_super_user(&mut tx, req).await?;
        log::info!(
            "seeded default super user admin/admin; change it via change-super-user-password in production"
        );
    }
    tx.commit().await.change_context_lazy(make_error)?;
    Ok(())
}
