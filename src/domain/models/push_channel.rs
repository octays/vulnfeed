use modql::field::Fields;
use nutype::nutype;
use sea_query::Value;
use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, sqlx::FromRow)]
pub struct PushChannel {
    pub id: i64,
    pub name: String,
    pub channel_type: String,
    pub access_token: String,
    pub secret_token: String,
    pub status: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Fields)]
pub struct CreatePushChannelRequest {
    pub name: PushChannelName,
    pub channel_type: PushChannelType,
    pub access_token: PushAccessToken,
    pub secret_token: PushSecretToken,
    pub status: bool,
}

fn valid_channel_type(s: &str) -> bool {
    matches!(s, "dingtalk" | "wecom" | "feishu")
}

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 50),
    derive(
        Clone, Debug, Display, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Borrow, TryFrom,
        Serialize
    )
)]
pub struct PushChannelName(String);

#[nutype(
    sanitize(trim, lowercase),
    validate(predicate = valid_channel_type),
    derive(
        Clone, Debug, Display, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Borrow, TryFrom,
        Serialize
    )
)]
pub struct PushChannelType(String);

#[nutype(
    sanitize(trim),
    validate(not_empty, len_char_max = 256),
    derive(
        Clone, Debug, Display, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Borrow, TryFrom,
        Serialize
    )
)]
pub struct PushAccessToken(String);

/// 可选密钥:企微无、钉钉/飞书加签可选,允许为空
#[nutype(
    sanitize(trim),
    validate(len_char_max = 256),
    derive(
        Clone, Debug, Display, PartialEq, Eq, PartialOrd, Ord, Hash, AsRef, Deref, Borrow, TryFrom,
        Serialize
    )
)]
pub struct PushSecretToken(String);

macro_rules! impl_value {
    ($t:ty) => {
        impl From<$t> for Value {
            fn from(value: $t) -> Self {
                Value::String(Some(Box::new(value.into_inner())))
            }
        }
    };
}
impl_value!(PushChannelName);
impl_value!(PushChannelType);
impl_value!(PushAccessToken);
impl_value!(PushSecretToken);
