// src-tauri/src/query_engine.rs
//
// Query engine: one call returns a page of rows plus the total match count,
// with optional filters, full-text search and multi-column sort.
//
// Safety: column names are checked against the table's real columns and
// quoted; every user value is passed as a bound parameter, never pasted
// into SQL.
//
// Speed:
// - No filters/search/sort -> fast path: rowid range scan (constant time at
//   any depth).
// - Otherwise -> WHERE / ORDER BY / LIMIT / OFFSET. A rowid tie-breaker keeps
//   page boundaries stable. Total counts are cached per (database, query) so
//   scrolling doesn't re-count.

use duckdb::types::Value;
use duckdb::{params_from_iter, Connection};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use crate::names::quote_ident;
use crate::query::{self, ColumnInfo};

pub const MAX_PAGE: u64 = 5_000;
const DEFAULT_LIMIT: u64 = 100;
const COUNT_CACHE_MAX: usize = 512;

// ---------------------------------------------------------------- model

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct QueryModel {
    #[serde(default)]
    pub offset: u64,
    #[serde(default = "default_limit")]
    pub limit: u64,
    #[serde(default)]
    pub sort: Vec<SortField>,
    /// field name -> condition. One condition per field; multiple fields are ANDed.
    #[serde(default)]
    pub filters: BTreeMap<String, FilterCondition>,
    #[serde(default)]
    pub search: Option<String>,
}

fn default_limit() -> u64 {
    DEFAULT_LIMIT
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SortField {
    pub field: String,
    pub direction: SortDirection,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    Asc,
    Desc,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FilterCondition {
    pub condition: FilterOp,
    /// `null` (or "") with == / != means IS NULL / IS NOT NULL.
    #[serde(default)]
    pub value: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum FilterOp {
    #[serde(rename = "==")]
    Eq,
    #[serde(rename = "!=")]
    Ne,
    #[serde(rename = ">")]
    Gt,
    #[serde(rename = "<")]
    Lt,
    #[serde(rename = ">=")]
    Gte,
    #[serde(rename = "<=")]
    Lte,
    #[serde(rename = "contains")]
    Contains,
}

impl FilterOp {
    fn sql(self) -> &'static str {
        match self {
            FilterOp::Eq => "=",
            FilterOp::Ne => "<>",
            FilterOp::Gt => ">",
            FilterOp::Lt => "<",
            FilterOp::Gte => ">=",
            FilterOp::Lte => "<=",
            FilterOp::Contains => "ILIKE",
        }
    }
}

#[derive(Serialize, Clone, Debug)]
pub struct QueryResult {
    pub rows: Vec<Vec<Option<String>>>,
    pub total_count: u64,
}

// ---------------------------------------------------------------- count cache

/// Remembers total counts for filtered queries. Cheap to clone.
/// Invalidate a database's entries after anything changes its tables.
#[derive(Clone, Default)]
pub struct CountCache(Arc<Mutex<HashMap<String, u64>>>);

impl CountCache {
    fn get(&self, key: &str) -> Option<u64> {
        self.0.lock().ok()?.get(key).copied()
    }
    fn put(&self, key: String, n: u64) {
        if let Ok(mut m) = self.0.lock() {
            if m.len() >= COUNT_CACHE_MAX {
                m.clear();
            }
            m.insert(key, n);
        }
    }
    /// Forget every count for one database (scope = its file path).
    pub fn invalidate(&self, scope: &str) {
        if let Ok(mut m) = self.0.lock() {
            let prefix = format!("{scope}\u{1}");
            m.retain(|k, _| !k.starts_with(&prefix));
        }
    }
}

// ---------------------------------------------------------------- execution

/// Execute without count caching (the spec-level API; commands use the cached variant).
#[allow(dead_code)]
pub fn execute_query(conn: &Connection, table: &str, model: &QueryModel) -> Result<QueryResult, String> {
    execute_query_cached(conn, table, model, None)
}

/// `cache` = (cache, scope) where scope identifies the database (its path).
pub fn execute_query_cached(
    conn: &Connection,
    table: &str,
    model: &QueryModel,
    cache: Option<(&CountCache, &str)>,
) -> Result<QueryResult, String> {
    let columns = query::columns(conn, table)?; // validates the table
    let limit = model.limit.min(MAX_PAGE);
    let select_list = columns
        .iter()
        .map(|c| format!("CAST({} AS VARCHAR)", quote_ident(&c.name)))
        .collect::<Vec<_>>()
        .join(", ");
    let from = quote_ident(table);

    let search = model.search.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let plain = model.filters.is_empty() && model.sort.is_empty() && search.is_none();

    // ---- fast path: whole table in natural order
    if plain {
        let sql = format!(
            "SELECT {select_list} FROM {from} WHERE rowid >= ? AND rowid < ? ORDER BY rowid"
        );
        let end = model.offset.saturating_add(limit);
        let rows = fetch(conn, &sql, &[Value::BigInt(model.offset as i64), Value::BigInt(end as i64)], columns.len())?;
        let total_count = query::row_count(conn, table)?;
        return Ok(QueryResult { rows, total_count });
    }

    // ---- general path
    let (where_sql, params) = build_where(conn, &columns, &model.filters, search)?;
    let order_sql = build_order(&columns, &model.sort)?;

    let sql = format!(
        "SELECT {select_list} FROM {from}{where_sql} ORDER BY {order_sql} LIMIT {limit} OFFSET {}",
        model.offset
    );
    let rows = fetch(conn, &sql, &params, columns.len())?;

    let count_sql = format!("SELECT count(*) FROM {from}{where_sql}");
    let key = cache.map(|(_, scope)| format!("{scope}\u{1}{table}\u{1}{count_sql}\u{1}{params:?}"));
    let cached = match (&key, cache) {
        (Some(k), Some((c, _))) => c.get(k),
        _ => None,
    };
    let total_count = match cached {
        Some(n) => n,
        None => {
            let n: i64 = conn
                .query_row(&count_sql, params_from_iter(params.iter()), |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if let (Some(k), Some((c, _))) = (key, cache) {
                c.put(k, n as u64);
            }
            n as u64
        }
    };

    Ok(QueryResult { rows, total_count })
}

fn fetch(conn: &Connection, sql: &str, params: &[Value], ncols: usize) -> Result<Vec<Vec<Option<String>>>, String> {
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params_from_iter(params.iter()), |r| {
            (0..ncols).map(|i| r.get::<_, Option<String>>(i)).collect()
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- SQL builders

fn find_column<'a>(columns: &'a [ColumnInfo], field: &str) -> Result<&'a ColumnInfo, String> {
    columns
        .iter()
        .find(|c| c.name == field)
        .or_else(|| columns.iter().find(|c| c.name.eq_ignore_ascii_case(field)))
        .ok_or_else(|| format!("Unknown column '{field}'"))
}

fn is_text(c: &ColumnInfo) -> bool {
    c.sql_type.to_uppercase().starts_with("VARCHAR")
}

/// Type names come from DuckDB's catalog, but check anyway before inlining.
fn safe_type(t: &str) -> bool {
    !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || " (),_".contains(c))
}

fn like_pattern(v: &str) -> String {
    let escaped = v.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

fn build_where(
    conn: &Connection,
    columns: &[ColumnInfo],
    filters: &BTreeMap<String, FilterCondition>,
    search: Option<&str>,
) -> Result<(String, Vec<Value>), String> {
    let mut parts: Vec<String> = Vec::new();
    let mut params: Vec<Value> = Vec::new();

    for (field, cond) in filters {
        let col = find_column(columns, field)?;
        let ident = quote_ident(&col.name);
        let value = cond.value.as_deref().filter(|v| !v.is_empty());

        match (cond.condition, value) {
            (FilterOp::Eq, None) => parts.push(format!("{ident} IS NULL")),
            (FilterOp::Ne, None) => parts.push(format!("{ident} IS NOT NULL")),
            (_, None) => return Err(format!("Filter on '{}' needs a value", col.name)),
            (FilterOp::Contains, Some(v)) => {
                parts.push(format!("CAST({ident} AS VARCHAR) ILIKE ? ESCAPE '\\'"));
                params.push(Value::Text(like_pattern(v)));
            }
            (op, Some(v)) if is_text(col) || !safe_type(&col.sql_type) => {
                parts.push(format!("{ident} {} ?", op.sql()));
                params.push(Value::Text(v.to_string()));
            }
            (op, Some(v)) => {
                check_castable(conn, v, col)?;
                parts.push(format!("{ident} {} CAST(? AS {})", op.sql(), col.sql_type));
                params.push(Value::Text(v.to_string()));
            }
        }
    }

    if let Some(term) = search {
        let mut targets: Vec<&ColumnInfo> = columns.iter().filter(|c| is_text(c)).collect();
        if targets.is_empty() {
            targets = columns.iter().collect(); // no text columns: search everything as text
        }
        let ors: Vec<String> = targets
            .iter()
            .map(|c| format!("CAST({} AS VARCHAR) ILIKE ? ESCAPE '\\'", quote_ident(&c.name)))
            .collect();
        for _ in &targets {
            params.push(Value::Text(like_pattern(term)));
        }
        parts.push(format!("({})", ors.join(" OR ")));
    }

    let sql = if parts.is_empty() { String::new() } else { format!(" WHERE {}", parts.join(" AND ")) };
    Ok((sql, params))
}

/// Clear message for "Amount >= abc" instead of a raw conversion error.
fn check_castable(conn: &Connection, v: &str, col: &ColumnInfo) -> Result<(), String> {
    conn.query_row(&format!("SELECT CAST(? AS {})", col.sql_type), [v], |_| Ok(()))
        .map_err(|_| format!("'{v}' is not a valid {} value for column '{}'", col.sql_type, col.name))
}

fn build_order(columns: &[ColumnInfo], sort: &[SortField]) -> Result<String, String> {
    let mut parts = Vec::with_capacity(sort.len() + 1);
    for s in sort {
        let col = find_column(columns, &s.field)?;
        let dir = match s.direction {
            SortDirection::Asc => "ASC",
            SortDirection::Desc => "DESC",
        };
        parts.push(format!("{} {dir} NULLS LAST", quote_ident(&col.name)));
    }
    parts.push("rowid ASC".into()); // stable tie-breaker for paging
    Ok(parts.join(", "))
}
