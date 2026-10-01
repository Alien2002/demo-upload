// src-tauri/src/staging.rs
//
// Staged import: the file is first loaded into a hidden text-only table
// (schema "_staging", invisible to list_tables). The user then reviews and
// edits column names and types; DuckDB checks every row for values that
// won't convert; commit builds the real table with ONE SQL statement.
//
//   stage (import.rs)  ->  profile  ->  check_column*  ->  commit | discard
//
// All conversion happens in SQL via TRY_CAST / TRY_STRPTIME, so a value that
// can't convert becomes NULL instead of failing the import. check_column
// reports exactly how many values that affects, before anything is committed.

use duckdb::Connection;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::names::{now_ms, quote_ident, quote_literal, table_name_from_path, unique_name};
use crate::query::{self, TableMeta};

pub const SCHEMA: &str = "_staging";
const SAMPLE_COUNT: usize = 5;
/// Rows used to SUGGEST types. Exact counts come from check_column on all rows.
const PROFILE_SAMPLE_ROWS: usize = 50_000;
/// A type is suggested if at least this share of non-blank values fit it; the
/// rest show up as "won't convert" in the Columns step instead of forcing Text.
const FIT_RATIO: f64 = 0.99;
const EXAMPLE_COUNT: usize = 5;

/// Date formats tried when a text column isn't ISO dates. Order matters:
/// day-first comes before month-first (East African / UK convention).
const DATE_FORMATS: &[&str] = &[
    "%d/%m/%Y", "%m/%d/%Y", "%d-%m-%Y", "%d.%m.%Y", "%Y/%m/%d", "%d-%b-%Y", "%d %b %Y", "%d %B %Y",
];
const TIME_SUFFIXES: &[&str] = &[" %H:%M:%S", " %H:%M"];

// ---------------------------------------------------------------- types

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Text,
    Integer,  // BIGINT
    Number,   // DOUBLE
    Currency, // DECIMAL(18,2) — exact sums for money
    Date,
    Datetime, // TIMESTAMP
    Boolean,
}

impl ColumnKind {
    pub fn sql_type(self) -> &'static str {
        match self {
            ColumnKind::Text => "VARCHAR",
            ColumnKind::Integer => "BIGINT",
            ColumnKind::Number => "DOUBLE",
            ColumnKind::Currency => "DECIMAL(18,2)",
            ColumnKind::Date => "DATE",
            ColumnKind::Datetime => "TIMESTAMP",
            ColumnKind::Boolean => "BOOLEAN",
        }
    }
}

/// What the profile suggests for one staged column.
#[derive(Serialize, Clone, Debug)]
pub struct StagedColumn {
    pub source: String,
    pub suggested_name: String,
    pub suggested_kind: ColumnKind,
    pub suggested_format: Option<String>,
    pub samples: Vec<String>,
    pub blank_count: u64,
    /// Excel error cells (#N/A, #DIV/0!, …) that were staged as blank.
    pub excel_error_count: u64,
}

#[derive(Serialize, Clone, Debug)]
pub struct StagedImport {
    pub staging_id: String,
    pub file_path: String,
    pub sheet: Option<String>,
    pub row_count: u64,
    pub suggested_table: String,
    pub columns: Vec<StagedColumn>,
}

/// The user's decision for one column.
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct ColumnSpec {
    pub source: String,
    pub name: String,
    pub kind: ColumnKind,
    #[serde(default)]
    pub format: Option<String>,
    #[serde(default = "yes")]
    pub include: bool,
}

fn yes() -> bool {
    true
}

#[derive(Serialize, Clone, Debug)]
pub struct ColumnCheck {
    /// Non-blank values that would become blank (NULL) with this type.
    pub invalid_count: u64,
    /// A few distinct values that fail.
    pub examples: Vec<String>,
    /// (original, converted) for the first few non-blank values.
    pub preview: Vec<(String, Option<String>)>,
    /// Currency only: values with more than 2 decimals that will be rounded.
    pub rounded_count: u64,
    /// Number types only: values like "00123" whose leading zeros will be lost.
    pub leading_zero_count: u64,
}

// ---------------------------------------------------------------- ids & housekeeping

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn new_staging_id() -> String {
    format!("s{}_{}", now_ms(), COUNTER.fetch_add(1, Ordering::SeqCst))
}

/// Staging ids come from the frontend: only accept the shape we generate.
pub fn validate_id(id: &str) -> Result<(), String> {
    let ok = id.len() > 1
        && id.starts_with('s')
        && id[1..].chars().all(|c| c.is_ascii_digit() || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!("Invalid staging id '{id}'"))
    }
}

pub fn staged_ref(id: &str) -> String {
    format!("{}.{}", quote_ident(SCHEMA), quote_ident(id))
}

pub fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(&format!("CREATE SCHEMA IF NOT EXISTS {}", quote_ident(SCHEMA)))
        .map_err(|e| e.to_string())
}

pub fn list_staged(conn: &Connection) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT table_name FROM information_schema.tables WHERE table_schema = ?")
        .map_err(|e| e.to_string())?;
    let rows = stmt.query_map([SCHEMA], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

pub fn discard(conn: &Connection, id: &str) -> Result<(), String> {
    validate_id(id)?;
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {}", staged_ref(id)))
        .map_err(|e| e.to_string())
}

/// Drop staging tables left behind (crash, closed window) except `keep`.
pub fn cleanup(conn: &Connection, keep: &[String]) -> Result<usize, String> {
    let mut dropped = 0;
    for id in list_staged(conn)? {
        if !keep.contains(&id) && validate_id(&id).is_ok() {
            discard(conn, &id)?;
            dropped += 1;
        }
    }
    Ok(dropped)
}

fn staged_columns(conn: &Connection, id: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT column_name FROM information_schema.columns
             WHERE table_schema = ? AND table_name = ? ORDER BY ordinal_position",
        )
        .map_err(|e| e.to_string())?;
    let cols: Vec<String> = stmt
        .query_map([SCHEMA, id], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;
    if cols.is_empty() {
        return Err("This import is no longer available (it was cancelled or already finished). Choose the file again.".into());
    }
    Ok(cols)
}

// ---------------------------------------------------------------- conversion SQL

/// "1,234.50" -> "1234.50" only when commas are real thousands separators.
fn clean_number(src: &str) -> String {
    format!(
        "CASE WHEN NOT contains({src}, ',') THEN trim({src}) \
              WHEN regexp_full_match(trim({src}), '[+-]?[0-9]{{1,3}}(,[0-9]{{3}})+([.][0-9]+)?') \
              THEN replace(trim({src}), ',', '') ELSE trim({src}) END"
    )
}

/// SQL expression converting the staged text column to the chosen type.
/// Values that don't fit become NULL.
pub fn conversion_expr(source: &str, kind: ColumnKind, format: Option<&str>) -> String {
    let src = quote_ident(source);
    let fmt = format.map(str::trim).filter(|f| !f.is_empty());
    match kind {
        ColumnKind::Text => src,
        // DuckDB ROUNDS '12.5' to 13 when casting to BIGINT, so only accept
        // integer-looking text ("12", "-7", "1,200", "12.00").
        ColumnKind::Integer => {
            let clean = clean_number(&src);
            format!(
                "CASE WHEN regexp_full_match({clean}, '[+-]?[0-9]+([.]0+)?') \
                 THEN TRY_CAST({clean} AS BIGINT) END"
            )
        }
        ColumnKind::Number | ColumnKind::Currency => {
            format!("TRY_CAST({} AS {})", clean_number(&src), kind.sql_type())
        }
        ColumnKind::Boolean => format!("TRY_CAST(trim({src}) AS BOOLEAN)"),
        ColumnKind::Date => match fmt {
            Some(f) => format!("CAST(TRY_STRPTIME(trim({src}), {}) AS DATE)", quote_literal(f)),
            None => format!("TRY_CAST(trim({src}) AS DATE)"),
        },
        ColumnKind::Datetime => match fmt {
            Some(f) => format!("TRY_STRPTIME(trim({src}), {})", quote_literal(f)),
            None => format!("TRY_CAST(trim({src}) AS TIMESTAMP)"),
        },
    }
}

/// A number-like code with leading zeros ("00123", "007.5") — NOT dates like "01/02/2024".
fn leading_zero(src: &str) -> String {
    format!("regexp_full_match(trim({src}), '[+-]?0[0-9]+([.][0-9]*)?')")
}

fn non_blank(src: &str) -> String {
    format!("({src} IS NOT NULL AND trim({src}) <> '')")
}

// ---------------------------------------------------------------- profile

/// Suggest a name, type (and date format) for every staged column.
///
/// Types are suggested from a random sample (fast); a type is suggested when
/// at least FIT_RATIO of the sampled non-blank values convert. Values with a
/// leading zero (codes like 00123) always mean Text. Blank counts are exact.
pub fn profile(
    conn: &Connection,
    id: &str,
    excel_errors: &[u64],
) -> Result<Vec<StagedColumn>, String> {
    let cols = staged_columns(conn, id)?;
    let table = staged_ref(id);

    // Exact non-blank counts over all rows (cheap: no casts or regexes).
    let nb_aggs: Vec<String> = cols
        .iter()
        .map(|c| format!("count(*) FILTER (WHERE {})", non_blank(&quote_ident(c))))
        .collect();
    let (total, non_blank_all): (i64, Vec<i64>) = conn
        .query_row(&format!("SELECT count(*), {} FROM {table}", nb_aggs.join(", ")), [], |r| {
            let total: i64 = r.get(0)?;
            let v = (0..cols.len()).map(|i| r.get::<_, i64>(i + 1)).collect::<Result<Vec<_>, _>>()?;
            Ok((total, v))
        })
        .map_err(|e| format!("Could not analyse columns: {e}"))?;

    // Type tests on a sample: nb, leading_zero, int, num, bool, date, ts.
    const TESTS: usize = 7;
    let sample = format!(
        "(SELECT * FROM {table} USING SAMPLE reservoir({PROFILE_SAMPLE_ROWS} ROWS) REPEATABLE (42))"
    );
    let mut aggs = Vec::with_capacity(cols.len() * TESTS);
    for c in &cols {
        let s = quote_ident(c);
        let nb = non_blank(&s);
        aggs.push(format!("count(*) FILTER (WHERE {nb})"));
        aggs.push(format!("count(*) FILTER (WHERE {nb} AND {})", leading_zero(&s)));
        for kind in [ColumnKind::Integer, ColumnKind::Number, ColumnKind::Boolean, ColumnKind::Date, ColumnKind::Datetime] {
            // Never suggest DATE for values that carry a time: it would drop the time.
            let guard = if kind == ColumnKind::Date { format!(" AND strpos({s}, ':') = 0") } else { String::new() };
            aggs.push(format!(
                "count(*) FILTER (WHERE {nb}{guard} AND {} IS NOT NULL)",
                conversion_expr(c, kind, None)
            ));
        }
    }
    let counts: Vec<i64> = conn
        .query_row(&format!("SELECT {} FROM {sample}", aggs.join(", ")), [], |r| {
            (0..aggs.len()).map(|i| r.get::<_, i64>(i)).collect()
        })
        .map_err(|e| format!("Could not analyse columns: {e}"))?;

    let mut out = Vec::with_capacity(cols.len());
    for (i, c) in cols.iter().enumerate() {
        let k = &counts[i * TESTS..(i + 1) * TESTS];
        let (nb, lead0, int, num, boolean, date, ts) = (k[0], k[1], k[2], k[3], k[4], k[5], k[6]);
        let fits = |n: i64| n > 0 && (n as f64) >= (nb as f64) * FIT_RATIO;
        let samples = samples(conn, &table, c)?;

        let (kind, format) = if nb == 0 || lead0 > 0 {
            (ColumnKind::Text, None) // empty, or codes like 00123 that must keep their zeros
        } else if fits(int) && int == num {
            (ColumnKind::Integer, None) // no fractional values among the numbers
        } else if fits(num) {
            (ColumnKind::Number, None)
        } else if fits(boolean) {
            (ColumnKind::Boolean, None)
        } else if fits(date) {
            (ColumnKind::Date, None)
        } else if fits(ts) {
            (ColumnKind::Datetime, None)
        } else if looks_like_dates(&samples) {
            detect_date_format(conn, &sample, c, nb)?.unwrap_or((ColumnKind::Text, None))
        } else {
            (ColumnKind::Text, None)
        };

        out.push(StagedColumn {
            source: c.clone(),
            suggested_name: c.trim().to_string(),
            suggested_kind: kind,
            suggested_format: format,
            samples,
            blank_count: (total - non_blank_all[i]).max(0) as u64,
            excel_error_count: excel_errors.get(i).copied().unwrap_or(0),
        });
    }
    Ok(out)
}

fn samples(conn: &Connection, table: &str, column: &str) -> Result<Vec<String>, String> {
    let s = quote_ident(column);
    let sql = format!(
        "SELECT {s} FROM {table} WHERE {} ORDER BY rowid LIMIT {SAMPLE_COUNT}",
        non_blank(&s)
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
    rows.collect::<Result<_, _>>().map_err(|e| e.to_string())
}

fn looks_like_dates(samples: &[String]) -> bool {
    !samples.is_empty()
        && samples.iter().all(|s| {
            let t = s.trim();
            t.len() >= 6 && t.chars().next().is_some_and(|c| c.is_ascii_digit()) && t.chars().any(|c| "/-. ".contains(c))
        })
}

/// Try common formats on the sample; the best one wins if it fits FIT_RATIO.
fn detect_date_format(
    conn: &Connection,
    table: &str,
    column: &str,
    non_blank_count: i64,
) -> Result<Option<(ColumnKind, Option<String>)>, String> {
    let mut candidates: Vec<(ColumnKind, String)> = Vec::new();
    for f in DATE_FORMATS {
        candidates.push((ColumnKind::Date, f.to_string()));
    }
    for f in DATE_FORMATS {
        for t in TIME_SUFFIXES {
            candidates.push((ColumnKind::Datetime, format!("{f}{t}")));
        }
    }
    let s = quote_ident(column);
    let nb = non_blank(&s);
    let aggs: Vec<String> = candidates
        .iter()
        .map(|(k, f)| format!("count(*) FILTER (WHERE {nb} AND {} IS NOT NULL)", conversion_expr(column, *k, Some(f))))
        .collect();
    let hits: Vec<i64> = conn
        .query_row(&format!("SELECT {} FROM {table}", aggs.join(", ")), [], |r| {
            (0..aggs.len()).map(|i| r.get::<_, i64>(i)).collect()
        })
        .map_err(|e| e.to_string())?;
    // Best-fitting format, if it reaches FIT_RATIO (first wins on ties:
    // day-first before month-first).
    let mut best: Option<((ColumnKind, String), i64)> = None;
    for (cand, n) in candidates.into_iter().zip(hits) {
        if best.as_ref().map_or(true, |(_, b)| n > *b) {
            best = Some((cand, n));
        }
    }
    Ok(best
        .filter(|(_, n)| *n > 0 && (*n as f64) >= (non_blank_count as f64) * FIT_RATIO)
        .map(|((k, f), _)| (k, Some(f))))
}

// ---------------------------------------------------------------- check

/// How many values of `source` would not convert to `kind` (+ examples, preview).
pub fn check_column(
    conn: &Connection,
    id: &str,
    source: &str,
    kind: ColumnKind,
    format: Option<&str>,
) -> Result<ColumnCheck, String> {
    validate_id(id)?;
    let cols = staged_columns(conn, id)?;
    if !cols.iter().any(|c| c == source) {
        return Err(format!("Unknown column '{source}'"));
    }
    let table = staged_ref(id);
    let s = quote_ident(source);
    let expr = conversion_expr(source, kind, format);
    let failing = format!("{} AND ({expr}) IS NULL", non_blank(&s));

    let invalid: i64 = conn
        .query_row(&format!("SELECT count(*) FROM {table} WHERE {failing}"), [], |r| r.get(0))
        .map_err(|e| format!("Invalid format or type: {e}"))?;

    let mut examples = Vec::new();
    if invalid > 0 {
        let sql = format!("SELECT DISTINCT {s} FROM {table} WHERE {failing} LIMIT {EXAMPLE_COUNT}");
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        examples = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .collect::<Result<_, _>>()
            .map_err(|e| e.to_string())?;
    }

    let sql = format!(
        "SELECT {s}, CAST(({expr}) AS VARCHAR) FROM {table} WHERE {} ORDER BY rowid LIMIT 3",
        non_blank(&s)
    );
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let preview = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))
        .map_err(|e| e.to_string())?
        .collect::<Result<_, _>>()
        .map_err(|e| e.to_string())?;

    let rounded: i64 = if kind == ColumnKind::Currency {
        conn.query_row(
            &format!(
                "SELECT count(*) FROM {table} WHERE {} AND regexp_full_match({}, '[+-]?[0-9]*[.][0-9]{{3,}}')",
                non_blank(&s),
                clean_number(&s)
            ),
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?
    } else {
        0
    };

    let leading_zero: i64 = if matches!(kind, ColumnKind::Integer | ColumnKind::Number | ColumnKind::Currency) {
        conn.query_row(
            &format!("SELECT count(*) FROM {table} WHERE {} AND {}", non_blank(&s), leading_zero(&s)),
            [],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?
    } else {
        0
    };

    Ok(ColumnCheck {
        invalid_count: invalid as u64,
        examples,
        preview,
        rounded_count: rounded as u64,
        leading_zero_count: leading_zero as u64,
    })
}

// ---------------------------------------------------------------- commit

/// Create the real table from the staged one and drop the staging table.
pub fn commit(
    conn: &Connection,
    id: &str,
    table_name: &str,
    specs: &[ColumnSpec],
) -> Result<TableMeta, String> {
    validate_id(id)?;
    let staged = staged_columns(conn, id)?;

    let table = table_name.trim();
    if table.is_empty() {
        return Err("Table name cannot be empty".into());
    }
    let existing = query::list_tables(conn)?;
    if existing.iter().any(|t| t.eq_ignore_ascii_case(table)) {
        return Err(format!("A table named '{table}' already exists in this folder"));
    }

    let included: Vec<&ColumnSpec> = specs.iter().filter(|s| s.include).collect();
    if included.is_empty() {
        return Err("Select at least one column to import".into());
    }
    let mut seen: Vec<String> = Vec::new();
    for s in &included {
        let name = s.name.trim();
        if name.is_empty() {
            return Err(format!("Column '{}' needs a name", s.source));
        }
        if seen.iter().any(|n| n.eq_ignore_ascii_case(name)) {
            return Err(format!("Two columns are named '{name}'; names must be unique"));
        }
        if !staged.iter().any(|c| c == &s.source) {
            return Err(format!("Unknown source column '{}'", s.source));
        }
        seen.push(name.to_string());
    }

    let select: Vec<String> = included
        .iter()
        .map(|s| {
            format!(
                "CAST({} AS {}) AS {}",
                conversion_expr(&s.source, s.kind, s.format.as_deref()),
                s.kind.sql_type(),
                quote_ident(s.name.trim())
            )
        })
        .collect();

    // ORDER BY rowid keeps file order, so the new table's rowids are 0..n-1
    // and the grid's fast paging path works.
    let sql = format!(
        "CREATE TABLE main.{} AS SELECT {} FROM {} ORDER BY rowid",
        quote_ident(table),
        select.join(", "),
        staged_ref(id)
    );
    conn.execute_batch(&sql).map_err(|e| format!("Import failed: {e}"))?;
    discard(conn, id)?;
    query::table_meta(conn, table)
}

/// Default table name for a file in this database: ledger, ledger_2, ...
pub fn suggest_table_name(conn: &Connection, file_path: &str) -> Result<String, String> {
    Ok(unique_name(&table_name_from_path(file_path), &query::list_tables(conn)?))
}

/// Specs that accept every suggestion (used by one-step imports).
pub fn default_specs(columns: &[StagedColumn]) -> Vec<ColumnSpec> {
    columns
        .iter()
        .map(|c| ColumnSpec {
            source: c.source.clone(),
            name: c.suggested_name.clone(),
            kind: c.suggested_kind,
            format: c.suggested_format.clone(),
            include: true,
        })
        .collect()
}
