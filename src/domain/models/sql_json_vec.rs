use std::ops::{Deref, DerefMut};

use serde::{Deserialize, Serialize, ser::SerializeSeq};

/// A `Vec<String>` persisted as a JSON string in a SQLite `TEXT` column.
///
/// SQLite has no array type, so list fields (tags, reference links, ...)
/// are stored as JSON and decoded transparently on read.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
pub struct SqlJsonVecString(Vec<String>);

impl SqlJsonVecString {
    pub fn new(v: Vec<String>) -> Self {
        SqlJsonVecString(v)
    }

    pub fn into_vec(self) -> Vec<String> {
        self.0
    }
}

impl From<Vec<String>> for SqlJsonVecString {
    fn from(v: Vec<String>) -> Self {
        SqlJsonVecString(v)
    }
}

impl From<SqlJsonVecString> for Vec<String> {
    fn from(v: SqlJsonVecString) -> Self {
        v.0
    }
}

impl Deref for SqlJsonVecString {
    type Target = Vec<String>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for SqlJsonVecString {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl Serialize for SqlJsonVecString {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for item in &self.0 {
            seq.serialize_element(item)?;
        }
        seq.end()
    }
}

impl From<SqlJsonVecString> for sea_query::Value {
    fn from(v: SqlJsonVecString) -> Self {
        let json = serde_json::to_string(&v.0).unwrap_or_else(|_| "[]".to_string());
        sea_query::Value::String(Some(Box::new(json)))
    }
}

impl sqlx::Type<sqlx::sqlite::Sqlite> for SqlJsonVecString {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <&str as sqlx::Type<sqlx::sqlite::Sqlite>>::type_info()
    }

    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <&str as sqlx::Type<sqlx::sqlite::Sqlite>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::sqlite::Sqlite> for SqlJsonVecString {
    fn decode(value: sqlx::sqlite::SqliteValueRef<'r>) -> Result<Self, sqlx::error::BoxDynError> {
        let json: Option<String> =
            <Option<String> as sqlx::Decode<sqlx::sqlite::Sqlite>>::decode(value)?;
        let json = json.unwrap_or_else(|| "[]".to_string());
        Ok(SqlJsonVecString(serde_json::from_str(&json)?))
    }
}
