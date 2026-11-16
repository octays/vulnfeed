use hmac::{Hmac, Mac};
use poem::{
    Request, handler,
    http::StatusCode,
    web::{Data, Json},
};
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use serde_json::{Value, json};
use sha2::Sha256;

use crate::{
    cli::Ctx,
    domain::ports::VulnService,
    output::{
        db::entities::feishu_app,
        feishu_receipts::{self, CardCallback},
        receipts::ReceiptError,
    },
};

fn authenticate(req: &Request, config: &crate::config::settings::Config) -> Result<(), StatusCode> {
    // A separate, high-entropy bridge token is required; the endpoint is disabled when absent.
    let Some(expected) = crate::output::bridge_supervisor::configured_token(config) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    if expected.len() < 32 {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let provided = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    let mut mac = Hmac::<Sha256>::new_from_slice(expected.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(b"vulnfeed-feishu-bridge");
    let tag = mac.finalize().into_bytes();
    let mut check = Hmac::<Sha256>::new_from_slice(provided.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    check.update(b"vulnfeed-feishu-bridge");
    if check.verify_slice(&tag).is_err() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(())
}

#[handler]
pub async fn apps<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
) -> Result<Json<Value>, StatusCode> {
    authenticate(req, &state.config)?;
    let rows = feishu_app::Entity::find()
        .filter(feishu_app::Column::Enabled.eq(true))
        .all(&state.db.orm())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(
        json!({"apps": rows.into_iter().map(|row| json!({"app_id":row.app_id,"app_secret":row.app_secret,"revision":row.revision})).collect::<Vec<_>>()}),
    ))
}

#[handler]
pub async fn card_action<S: VulnService + Send + Sync + 'static>(
    state: Data<&Ctx<S>>,
    req: &Request,
    Json(callback): Json<CardCallback>,
) -> Result<Json<Value>, StatusCode> {
    authenticate(req, &state.config)?;
    let task_id = callback.event.action.value.task_id;
    let operator = callback.event.operator.open_id.clone();
    match feishu_receipts::handle_action(&state.db.orm(), callback).await {
        Ok(action) => {
            let card = match feishu_receipts::submitted_card(&state.db.orm(), task_id).await {
                Ok(card) => Some(json!({"type":"raw", "data":card})),
                Err(e) => {
                    log::error!("Failed to build submitted card for task {task_id}: {e}");
                    None
                }
            };
            let toast = match action {
                feishu_receipts::CardAction::Submitted => "回执已提交",
                feishu_receipts::CardAction::Deleted => "已删除你的回执",
            };
            // Enrich name/department right after answering (never inside the
            // 3-second callback window); the refresh loop then updates cards.
            let orm = state.db.orm();
            tokio::spawn(async move {
                if let Err(e) = crate::output::receipt_name_lookup::process_pending(&orm).await {
                    log::warn!("post-callback name/department resolve failed: {e}");
                }
                crate::output::receipt_refresh::refresh_task_now(&orm, task_id).await;
            });
            Ok(Json(
                json!({"toast":{"type":"success","content":toast},"card":card}),
            ))
        }
        // User-correctable rejections answer 200 with an error toast so the
        // responder sees the reason instead of Feishu's generic failure text.
        Err(
            e @ (ReceiptError::Unauthorized
            | ReceiptError::DeletionDisabled
            | ReceiptError::Conflict
            | ReceiptError::Duplicate
            | ReceiptError::AlreadyOpen(_)
            | ReceiptError::Invalid
            | ReceiptError::TaskClosed
            | ReceiptError::NoPending
            | ReceiptError::NoChats
            | ReceiptError::NoResponse
            | ReceiptError::Incomplete(_)),
        ) => {
            log::warn!("card action rejected (task {task_id}, operator {operator}): {e}");
            Ok(Json(
                json!({"toast":{"type":"error","content":e.to_string()}}),
            ))
        }
        Err(ReceiptError::Orm(e)) => {
            log::error!("Feishu receipt database error: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR)
        }
    }
}
