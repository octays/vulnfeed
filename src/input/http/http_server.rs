use std::{io, time::Duration};

use mea::{shutdown::ShutdownRecv, waitgroup::WaitGroup};
use poem::{
    Endpoint, EndpointExt, Route, get,
    listener::{Acceptor, Listener, TcpAcceptor, TcpListener},
    post, put,
};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "public/"]
struct Assets;

use crate::input::http::spa_endpoint::{SpaDirEndpoint, SpaFileEndpoint};

use crate::{
    cli::Ctx,
    domain::ports::VulnService,
    input::http::{
        handlers::{
            account, feishu_apps, feishu_bridge, feishu_directory, login, plugin, receipts,
            sec_notice, sync_data_task, tech_stack, vuln_information,
        },
        middleware::auth::AuthMiddleware,
    },
    utils::runtime::{self, Runtime},
};

pub(crate) type ServerFuture<T> = runtime::JoinHandle<Result<T, io::Error>>;

#[derive(Debug)]
pub struct ServerState {
    server_fut: ServerFuture<()>,
    shutdown_rx_server: ShutdownRecv,
}

impl ServerState {
    pub async fn await_shutdown(self) {
        self.shutdown_rx_server.is_shutdown().await;
        log::info!("http server is shutting down");

        match self.server_fut.await {
            Ok(_) => log::info!("http server stopped"),
            Err(err) => log::error!(err:?;"http server failed."),
        }
    }
}

pub async fn make_acceptor(listen_addr: &str) -> Result<TcpAcceptor, io::Error> {
    log::info!("listening on {}", listen_addr);
    let acceptor = TcpListener::bind(&listen_addr).into_acceptor().await?;
    Ok(acceptor)
}

pub async fn start_server<S: VulnService + Send + Sync + 'static>(
    ctx: Ctx<S>,
    rt: &Runtime,
    shutdown_rx: ShutdownRecv,
    acceptor: TcpAcceptor,
) -> Result<ServerState, io::Error> {
    let wg = WaitGroup::new();
    let shutdown_rx_server = shutdown_rx;
    let server_fut = {
        let wg_clone = wg.clone();
        let shutdown_clone = shutdown_rx_server.clone();
        let static_dir = ctx.config.server.static_dir.clone();
        let spa = if static_dir.is_empty() {
            SpaFileEndpoint::<Assets>::new().boxed()
        } else {
            SpaDirEndpoint::new(&static_dir).boxed()
        };
        let route = Route::new()
            .nest("/", spa)
            .nest("/api", api_routes::<S>())
            .data(ctx.clone());
        let listen_addr = acceptor.local_addr()[0].clone();
        let signal = async move {
            log::info!("server has started on [{listen_addr}]");
            drop(wg_clone);

            shutdown_clone.is_shutdown().await;
            log::info!("server is closing");
        };
        rt.spawn(async move {
            poem::Server::new_with_acceptor(acceptor)
                .run_with_graceful_shutdown(route, signal, Some(Duration::from_secs(10)))
                .await
        })
    };
    wg.await;
    Ok(ServerState {
        server_fut,
        shutdown_rx_server,
    })
}

fn api_routes<S: VulnService + Send + Sync + 'static>() -> impl Endpoint {
    Route::new()
        .nest("/login", post(login::login::<S>::default()))
        .at(
            "/internal/feishu/card-action",
            post(feishu_bridge::card_action::<S>::default()),
        )
        .at(
            "/internal/feishu/apps",
            get(feishu_bridge::apps::<S>::default()),
        )
        .nest(
            "/",
            Route::new()
                .nest(
                    "/vulns",
                    Route::new()
                        .at(
                            "",
                            get(vuln_information::list_vuln_information::<S>::default()),
                        )
                        .at(
                            "/:id",
                            get(vuln_information::get_vuln_information_detail::<S>::default()),
                        ),
                )
                .nest("/plugins", Route::new().at("", get(plugin::list_plugins)))
                .nest("/notices", Route::new().at("", get(plugin::list_notice)))
                .nest(
                    "/tech_stacks",
                    Route::new()
                        .at(
                            "",
                            get(tech_stack::list::<S>::default())
                                .post(tech_stack::create::<S>::default()),
                        )
                        .at(
                            "/:id",
                            put(tech_stack::update::<S>::default())
                                .delete(tech_stack::delete::<S>::default()),
                        )
                        .at("/reclassify", post(tech_stack::reclassify::<S>::default()))
                        .at(
                            "/llm-setting",
                            get(tech_stack::get_llm_setting::<S>::default())
                                .put(tech_stack::update_llm_setting::<S>::default()),
                        ),
                )
                .nest(
                    "/receipts",
                    Route::new()
                        .at(
                            "/apps",
                            get(feishu_apps::list::<S>::default())
                                .post(feishu_apps::create::<S>::default()),
                        )
                        .at("/apps/:id", put(feishu_apps::update::<S>::default()))
                        .at("/directory", get(feishu_directory::list::<S>::default()))
                        .at(
                            "/directory/members",
                            get(feishu_directory::list_members::<S>::default()),
                        )
                        .at(
                            "/directory/sync-members",
                            post(feishu_directory::sync_members::<S>::default()),
                        )
                        .at(
                            "/directory/chats",
                            post(feishu_directory::group::<S>::default()),
                        )
                        .at(
                            "/directory/people",
                            post(feishu_directory::person::<S>::default()),
                        )
                        .at(
                            "/directory/people/:id/identities",
                            post(feishu_directory::identity::<S>::default()),
                        )
                        .at(
                            "/directory/:kind/:id",
                            put(feishu_directory::update::<S>::default()),
                        )
                        .at(
                            "/cases",
                            get(receipts::list_cases::<S>::default())
                                .post(receipts::create_case::<S>::default()),
                        )
                        .at("/cases/:id", get(receipts::get_case::<S>::default()))
                        .at(
                            "/cases/:id/export",
                            get(receipts::export_case::<S>::default()),
                        )
                        .at("/people", get(receipts::list_people::<S>::default()))
                        .at("/trigger", post(receipts::trigger_case::<S>::default()))
                        .at(
                            "/cases/:id/close",
                            post(receipts::close_case::<S>::default()),
                        )
                        .at(
                            "/responses/:id/retry-name",
                            post(receipts::retry_response_name::<S>::default()),
                        )
                        .at(
                            "/responses/:id",
                            put(receipts::update_response::<S>::default())
                                .delete(receipts::delete_response::<S>::default()),
                        )
                        .at(
                            "/tasks/:id/progress",
                            post(receipts::progress::<S>::default()),
                        )
                        .at("/tasks/:id/send", post(receipts::send_task::<S>::default()))
                        .at(
                            "/tasks/:id/remind",
                            post(receipts::remind_task::<S>::default()),
                        )
                        .at(
                            "/dimensions",
                            get(receipts::list_dimensions::<S>::default()),
                        )
                        .at(
                            "/dimensions/:id",
                            put(receipts::update_dimension::<S>::default()),
                        ),
                )
                .nest(
                    "/sync_data_task",
                    Route::new()
                        .at(
                            "",
                            post(sync_data_task::create_or_update_sync_data_task::<S>::default())
                                .get(sync_data_task::get_sync_data_task::<S>::default()),
                        )
                        .at(
                            "/run",
                            post(sync_data_task::run_sync_data_task_now::<S>::default()),
                        ),
                )
                .nest(
                    "/account",
                    Route::new().at("/password", post(account::change_password::<S>::default())),
                )
                .nest(
                    "/sec_notice",
                    Route::new().at("", get(sec_notice::list_sec_notice::<S>::default())),
                )
                .with(AuthMiddleware::<S>::default()),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::settings::Config,
        domain::services::Service,
        output::{db::sqlite::Db, scheduler::Scheduler},
        utils::auth::jwt::JWT,
    };
    use poem::{
        Request,
        http::{Method, StatusCode},
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn receipt_page_endpoints_and_settings_are_reachable() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!("./migrations").run(&pool).await.unwrap();
        let db = Db::test_pool(pool);
        let config = Config::default();
        let jwt = JWT::new(&config.auth.jwt.secret);
        let token = jwt.generate_token(3600, 1, Default::default()).unwrap();
        let ctx = Ctx {
            vuln_service: Arc::new(Service::new(db.clone())),
            db: db.clone(),
            config: Arc::new(config),
            sched: Arc::new(Scheduler::try_new(db.clone()).await.unwrap()),
            jwt: Arc::new(jwt),
        };
        let api = api_routes::<Service<Db>>().data(ctx);
        for path in ["/receipts/cases", "/receipts/apps", "/receipts/directory"] {
            let req = Request::builder()
                .uri(path.parse().unwrap())
                .header("Authorization", format!("Bearer {token}"))
                .finish();
            let res = api.get_response(req).await;
            assert_eq!(res.status(), StatusCode::OK, "{path}");
        }
        let request = |method: Method, path: &str, body: &'static str| {
            Request::builder()
                .method(method)
                .uri(path.parse().unwrap())
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(body)
        };
        for (name, app_id) in [("应用一", "app-one"), ("应用二", "app-two")] {
            let body = format!(
                r#"{{"name":"{name}","app_id":"{app_id}","app_secret":"secret-value","tenant_key":"tenant"}}"#
            );
            let req = Request::builder()
                .method(Method::POST)
                .uri("/receipts/apps".parse().unwrap())
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(body);
            assert_eq!(api.get_response(req).await.status(), StatusCode::OK);
        }
        let body = api
            .get_response(request(Method::GET, "/receipts/apps", ""))
            .await
            .into_body()
            .into_string()
            .await
            .unwrap();
        assert!(body.contains("app-one") && body.contains("app-two"));
        assert!(!body.contains("secret-value"));
        assert_eq!(
            api.get_response(request(
                Method::PUT,
                "/receipts/apps/1",
                r#"{"name":"更新","app_id":"app-one","tenant_key":"tenant","app_secret":""}"#
            ))
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            api.get_response(request(
                Method::PUT,
                "/receipts/apps/1",
                r#"{"name":"更新","app_id":"other","tenant_key":"tenant","app_secret":""}"#
            ))
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        let secret: String = sqlx::query_scalar("SELECT app_secret FROM feishu_app WHERE id=1")
            .fetch_one(db.sqlx_pool())
            .await
            .unwrap();
        assert_eq!(secret, "secret-value");

        for (path, body) in [
            (
                "/receipts/directory/chats",
                r#"{"app_id":1,"name":"一群","chat_id":"oc_a"}"#,
            ),
            (
                "/receipts/directory/chats",
                r#"{"app_id":2,"name":"二群","chat_id":"oc_b"}"#,
            ),
            ("/receipts/directory/people", r#"{"name":"负责人"}"#),
            (
                "/receipts/directory/people/1/identities",
                r#"{"app_id":1,"open_id":"ou_a"}"#,
            ),
        ] {
            assert_eq!(
                api.get_response(request(Method::POST, path, body))
                    .await
                    .status(),
                StatusCode::OK,
                "{path}"
            );
        }
        assert_eq!(
            api.get_response(request(
                Method::POST,
                "/receipts/directory/people/1/identities",
                r#"{"app_id":2,"open_id":"ou_b"}"#
            ))
            .await
            .status(),
            StatusCode::OK
        );
        let data = api
            .get_response(request(Method::GET, "/receipts/directory", ""))
            .await
            .into_body()
            .into_string()
            .await
            .unwrap();
        let data: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(data["identities"].as_array().unwrap().len(), 2);
        let invalid = api
            .get_response(request(
                Method::POST,
                "/receipts/directory/people/1/identities",
                r#"{"app_id":1,"open_id":"zhengrc"}"#,
            ))
            .await;
        assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        assert!(
            invalid
                .into_body()
                .into_string()
                .await
                .unwrap()
                .contains("ou_")
        );
        sqlx::query("INSERT INTO vuln_information(key,title) VALUES('test','test')")
            .execute(db.sqlx_pool())
            .await
            .unwrap();
        let duplicate = api
            .get_response(request(
                Method::POST,
                "/receipts/cases",
                r#"{"source_type":"vuln","source_id":1}"#,
            ))
            .await;
        assert_eq!(duplicate.status(), StatusCode::OK);
        let duplicate: serde_json::Value =
            serde_json::from_str(&duplicate.into_body().into_string().await.unwrap()).unwrap();
        assert_eq!(duplicate["id"], 1);
        assert_eq!(duplicate["created"], true);
        let res = api
            .get_response(request(Method::GET, "/receipts/cases/1", ""))
            .await;
        assert_eq!(res.status(), StatusCode::OK);
        let detail: serde_json::Value =
            serde_json::from_str(&res.into_body().into_string().await.unwrap()).unwrap();
        assert_eq!(detail["tasks"][0]["refresh_failed"], false);
        sqlx::query("UPDATE feishu_chat SET enabled=0 WHERE id=1")
            .execute(db.sqlx_pool())
            .await
            .unwrap();
        sqlx::query("INSERT INTO security_notice(key,title) VALUES('notice','历史公告')")
            .execute(db.sqlx_pool())
            .await
            .unwrap();
        for (kind, expected_case, created) in [
            ("vuln", 1, false),
            ("notice", 2, true),
            ("notice", 2, false),
        ] {
            let body = format!(r#"{{"source_type":"{kind}","source_id":1}}"#);
            let req = Request::builder()
                .method(Method::POST)
                .uri("/receipts/trigger".parse().unwrap())
                .header("Authorization", format!("Bearer {token}"))
                .header("Content-Type", "application/json")
                .body(body);
            let response = api.get_response(req).await;
            assert_eq!(response.status(), StatusCode::OK);
            let response: serde_json::Value =
                serde_json::from_str(&response.into_body().into_string().await.unwrap()).unwrap();
            assert_eq!(response["case_id"], expected_case);
            assert_eq!(response["created"], created);
            assert_eq!(response["failures"].as_array().unwrap().len(), 1);
            assert!(response["sent_teams"].as_array().unwrap().is_empty());
        }
        sqlx::query("INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,display_name,status,details) VALUES(1,'app-one','tenant','ou_manual','测试人','affected','原说明')").execute(db.sqlx_pool()).await.unwrap();
        let unauth = Request::builder()
            .method(Method::PUT)
            .uri("/receipts/responses/1".parse().unwrap())
            .header("Content-Type", "application/json")
            .body(
                r#"{"version":1,"display_name":"测试人","status":"unaffected","details":"更新"}"#,
            );
        assert_eq!(
            api.get_response(unauth).await.status(),
            StatusCode::UNAUTHORIZED
        );
        let edited=api.get_response(request(Method::PUT,"/receipts/responses/1",r#"{"version":1,"display_name":"测试人","status":"unaffected","details":"更新说明"}"#)).await;
        assert_eq!(edited.status(), StatusCode::NO_CONTENT);
        let stale = api
            .get_response(request(
                Method::DELETE,
                "/receipts/responses/1",
                r#"{"version":1}"#,
            ))
            .await;
        assert_eq!(stale.status(), StatusCode::BAD_REQUEST);
        let deleted = api
            .get_response(request(
                Method::DELETE,
                "/receipts/responses/1",
                r#"{"version":2}"#,
            ))
            .await;
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
        let res = api
            .get_response(request(Method::GET, "/receipts/cases/1", ""))
            .await;
        let detail: serde_json::Value =
            serde_json::from_str(&res.into_body().into_string().await.unwrap()).unwrap();
        assert!(detail["responses"].as_array().unwrap().is_empty());
        sqlx::query("INSERT INTO receipt_response(task_id,app_id,tenant_key,operator_open_id,display_name,status,details) VALUES(1,'app-one','tenant','ou_pending','','unaffected','ok')").execute(db.sqlx_pool()).await.unwrap();
        let req = Request::builder()
            .method(Method::POST)
            .uri("/receipts/responses/2/retry-name".parse().unwrap())
            .finish();
        assert_eq!(
            api.get_response(req).await.status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            api.get_response(request(
                Method::POST,
                "/receipts/responses/2/retry-name",
                ""
            ))
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM receipt_name_lookup WHERE response_id=2")
                .fetch_one(db.sqlx_pool())
                .await
                .unwrap();
        assert_eq!(count, 1);
        let list = api
            .get_response(request(Method::GET, "/receipts/cases", ""))
            .await;
        let list: serde_json::Value =
            serde_json::from_str(&list.into_body().into_string().await.unwrap()).unwrap();
        let case = list["data"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["id"] == 1)
            .unwrap();
        assert_eq!(case["response_count"], 1);
        assert_eq!(case["unsent_teams"], 1);
    }
}
