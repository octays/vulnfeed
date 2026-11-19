use crate::{AppResult, errors::Error};
use error_stack::ResultExt;
use modql::{SIden, field::HasSeaFields};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseTransaction, FromQueryResult, Statement};
use sea_query::{
    Alias, Asterisk, Condition, Expr, Iden, IntoIden, OnConflict, Query, SelectStatement,
    SqliteQueryBuilder, TableRef, Values,
};

fn stmt(sql: String, values: Values) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Sqlite, sql, values.0)
}

pub trait Dao {
    const TABLE: &'static str;
    fn table_ref() -> TableRef {
        TableRef::Table(SIden(Self::TABLE).into_iden())
    }
}

#[derive(Iden)]
pub enum CommonIden {
    Id,
}

pub async fn dao_create<D, E>(tx: &mut DatabaseTransaction, req: E) -> AppResult<i64>
where
    E: HasSeaFields,
    D: Dao,
{
    let (columns, values) = req.not_none_sea_fields().for_sea_insert();
    let mut q = Query::insert();
    q.into_table(D::table_ref())
        .columns(columns)
        .values(values)
        .change_context_lazy(|| Error::Message("failed to create record".into()))?
        .returning(Query::returning().columns([CommonIden::Id]));
    let (sql, values) = q.build(SqliteQueryBuilder);
    let row = tx
        .query_one(stmt(sql, values))
        .await
        .change_context_lazy(|| Error::Message("failed to create record".into()))?
        .ok_or_else(|| Error::Message("missing created record".into()))?;
    row.try_get("", "id")
        .change_context_lazy(|| Error::Message("failed to read created id".into()))
}

pub async fn dao_first<D, T>(tx: &mut DatabaseTransaction) -> AppResult<Option<T>>
where
    D: Dao,
    T: FromQueryResult + Send + Sync,
{
    let (sql, values) = Query::select()
        .from(D::table_ref())
        .column(Asterisk)
        .build(SqliteQueryBuilder);
    T::find_by_statement(stmt(sql, values))
        .one(&*tx)
        .await
        .change_context_lazy(|| Error::Message("failed to fetch one record".into()))
}

pub async fn dao_upsert<D, E>(
    tx: &mut DatabaseTransaction,
    req: E,
    conflict_column: &str,
    update_columns: &[&str],
) -> AppResult<i64>
where
    E: HasSeaFields,
    D: Dao,
{
    let fields = req.not_none_sea_fields();
    let conflict_value = fields
        .clone()
        .into_vec()
        .into_iter()
        .find(|f| f.iden.to_string() == conflict_column)
        .and_then(|f| f.sea_value().cloned());
    let (columns, values) = fields.for_sea_insert();
    let mut q = Query::insert();
    q.into_table(D::table_ref())
        .columns(columns)
        .values(values)
        .change_context_lazy(|| Error::Message("failed to upsert record".into()))?;
    let conflict = if update_columns.is_empty() {
        OnConflict::column(Alias::new(conflict_column))
            .do_nothing()
            .to_owned()
    } else {
        let mut c = OnConflict::column(Alias::new(conflict_column));
        for col in update_columns {
            c = c.update_column(Alias::new(*col)).to_owned();
        }
        c
    };
    q.on_conflict(conflict)
        .returning(Query::returning().columns([CommonIden::Id]));
    let (sql, values) = q.build(SqliteQueryBuilder);
    if let Some(row) = tx
        .query_one(stmt(sql, values))
        .await
        .change_context_lazy(|| Error::Message("failed to upsert record".into()))?
    {
        return row
            .try_get("", "id")
            .change_context_lazy(|| Error::Message("failed to read upsert id".into()));
    }
    let value = conflict_value.ok_or_else(|| {
        Error::Message(format!(
            "conflict column '{conflict_column}' not found in upsert fields"
        ))
    })?;
    let (sql, values) = Query::select()
        .from(D::table_ref())
        .column(CommonIden::Id)
        .and_where(Expr::col(Alias::new(conflict_column)).eq(value))
        .build(SqliteQueryBuilder);
    let row = tx
        .query_one(stmt(sql, values))
        .await
        .change_context_lazy(|| Error::Message("failed to find existing upsert record".into()))?
        .ok_or_else(|| Error::Message("missing upsert record".into()))?;
    row.try_get("", "id")
        .change_context_lazy(|| Error::Message("failed to read upsert id".into()))
}

pub async fn dao_fetch_by_column<D, T>(
    tx: &mut DatabaseTransaction,
    column_name: &str,
    value: &str,
) -> AppResult<Option<T>>
where
    D: Dao,
    T: FromQueryResult + Send + Sync,
{
    let (sql, values) = Query::select()
        .from(D::table_ref())
        .column(Asterisk)
        .and_where(Expr::col(Alias::new(column_name)).eq(value))
        .build(SqliteQueryBuilder);
    T::find_by_statement(stmt(sql, values))
        .one(&*tx)
        .await
        .change_context_lazy(|| Error::Message("failed to fetch record by column".into()))
}

pub async fn dao_update<D, E>(tx: &mut DatabaseTransaction, id: i64, req: E) -> AppResult<u64>
where
    E: HasSeaFields,
    D: Dao,
{
    let (columns, values) = req.not_none_sea_fields().for_sea_insert();
    let mut q = Query::update();
    q.table(D::table_ref())
        .and_where(Expr::col(CommonIden::Id).eq(id));
    for (column, value) in columns.into_iter().zip(values) {
        q.value(column, value);
    }
    let (sql, values) = q.build(SqliteQueryBuilder);
    Ok(tx
        .execute(stmt(sql, values))
        .await
        .change_context_lazy(|| Error::Message("failed to update record".into()))?
        .rows_affected())
}

pub async fn dao_update_field<D: Dao>(
    tx: &mut DatabaseTransaction,
    id: i64,
    field_name: &str,
    field_value: impl Into<sea_query::Value>,
) -> AppResult<u64> {
    let mut q = Query::update();
    q.table(D::table_ref())
        .and_where(Expr::col(CommonIden::Id).eq(id))
        .value(Alias::new(field_name), field_value);
    let (sql, values) = q.build(SqliteQueryBuilder);
    Ok(tx
        .execute(stmt(sql, values))
        .await
        .change_context_lazy(|| Error::Message("failed to update field".into()))?
        .rows_affected())
}

pub async fn dao_fetch_by_id<D, T>(tx: &mut DatabaseTransaction, id: i64) -> AppResult<Option<T>>
where
    D: Dao,
    T: FromQueryResult + Send + Sync,
{
    let (sql, values) = Query::select()
        .from(D::table_ref())
        .column(Asterisk)
        .and_where(Expr::col(CommonIden::Id).eq(id))
        .build(SqliteQueryBuilder);
    T::find_by_statement(stmt(sql, values))
        .one(&*tx)
        .await
        .change_context_lazy(|| Error::Message("failed to fetch record by id".into()))
}

pub struct DaoQueryBuilder<D: Dao> {
    query: SelectStatement,
    conditions: Vec<Condition>,
    _phantom: std::marker::PhantomData<D>,
}

impl<D: Dao> Default for DaoQueryBuilder<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D: Dao> DaoQueryBuilder<D> {
    pub fn new() -> Self {
        let mut query = Query::select();
        query.from(D::table_ref()).column(Asterisk);

        Self {
            query,
            conditions: Vec::new(),
            _phantom: std::marker::PhantomData,
        }
    }

    pub fn and_where_like(mut self, column: &str, value: &str) -> Self {
        let condition = Expr::col(Alias::new(column)).like(format!("%{}%", value));
        self.query.and_where(condition.clone());
        self.conditions.push(Condition::all().add(condition));
        self
    }

    pub fn and_where_eq(mut self, column: &str, value: i64) -> Self {
        let condition = Expr::col(Alias::new(column)).eq(value);
        self.query.and_where(condition.clone());
        self.conditions.push(Condition::all().add(condition));
        self
    }

    pub fn and_where_in(mut self, column: &str, values: &[i64]) -> Self {
        if !values.is_empty() {
            let condition = Expr::col(Alias::new(column)).is_in(values.iter().copied());
            self.query.and_where(condition.clone());
            self.conditions.push(Condition::all().add(condition));
        }
        self
    }

    pub fn and_where_bool(mut self, column: &str, value: bool) -> Self {
        let condition = Expr::col(Alias::new(column)).eq(value);
        self.query.and_where(condition.clone());
        self.conditions.push(Condition::all().add(condition));
        self
    }

    pub fn order_by_desc(mut self, column: &str) -> Self {
        self.query
            .order_by(Alias::new(column), sea_query::Order::Desc);
        self
    }

    pub fn order_by_asc(mut self, column: &str) -> Self {
        self.query
            .order_by(Alias::new(column), sea_query::Order::Asc);
        self
    }

    /// Order by an arbitrary expression, e.g. emptiness-aware date sorting.
    pub fn order_by_expr(mut self, expr: sea_query::SimpleExpr, order: sea_query::Order) -> Self {
        self.query.order_by_expr(expr, order);
        self
    }

    pub fn limit_offset(mut self, limit: i64, offset: i64) -> Self {
        self.query.limit(limit as u64).offset(offset as u64);
        self
    }

    pub async fn fetch_all<T>(self, tx: &mut DatabaseTransaction) -> AppResult<Vec<T>>
    where
        T: FromQueryResult + Send + Sync,
    {
        let (sql, values) = self.query.build(SqliteQueryBuilder);
        T::find_by_statement(stmt(sql, values))
            .all(&*tx)
            .await
            .change_context_lazy(|| Error::Message("failed to fetch records".into()))
    }

    pub async fn count(self, tx: &mut DatabaseTransaction) -> AppResult<i64> {
        let mut query = Query::select();
        query
            .from(D::table_ref())
            .expr_as(Expr::col(CommonIden::Id).count(), Alias::new("total"));
        for condition in self.conditions {
            query.cond_where(condition);
        }
        let (sql, values) = query.build(SqliteQueryBuilder);
        let row = tx
            .query_one(stmt(sql, values))
            .await
            .change_context_lazy(|| Error::Message("failed to count records".into()))?
            .ok_or_else(|| Error::Message("missing count result".into()))?;
        row.try_get("", "total")
            .change_context_lazy(|| Error::Message("failed to read count".into()))
    }
}
