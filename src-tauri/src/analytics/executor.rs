// src-tauri/src/analytics/executor.rs
//
// run_command: text -> tokens -> Command -> plan -> SQL -> result.
//
//   Scalars (COUNT, TOTAL):   one SELECT, values returned and saved as variables.
//   Tables  (EXTRACT, SORT, SUMMARIZE):
//       without TO -> _results.r_<n>   (session result, chainable)
//       with TO    -> main.<name>      (permanent table in the sidebar)
//
// Every run, successful or not, is written to the folder's command log.

use duckdb::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::binder::{Binder, Ty, Variable, Variables};
use super::commands::{parse_command, PlanContext};
use super::dsl::{lexer, parser::Parser};
use super::error::{CmdResult, CommandError};
use super::plan::{Output, PlannedCommand};
use super::results::{self, NewLogEntry};
use super::sqlgen;
use crate::names::{quote_ident, quote_literal};
use crate::query::{self, TableMeta, TableRef};

// ---------------------------------------------------------------- state

/// Per-folder session state: variables, and whether old results were cleared.
#[derive(Clone, Default)]
pub struct AnalyticsState {
    inner: Arc<Mutex<StateInner>>,
}

#[derive(Default)]
struct StateInner {
    vars: HashMap<String, Variables>,
    started: HashSet<String>,
}

impl AnalyticsState {
    fn variables(&self, scope: &str) -> Variables {
        self.inner.lock().map(|s| s.vars.get(scope).cloned().unwrap_or_default()).unwrap_or_default()
    }

    fn set_variables(&self, scope: &str, set: Vec<(String, Variable)>) {
        if let Ok(mut s) = self.inner.lock() {
            let vars = s.vars.entry(scope.to_string()).or_default();
            for (k, v) in set {
                vars.insert(k, v);
            }
        }
    }

    /// First use of a folder in this app session: create schemas, clear old results.
    fn start_session(&self, scope: &str, conn: &Connection) -> Result<(), String> {
        let first = self.inner.lock().map(|mut s| s.started.insert(scope.to_string())).unwrap_or(true);
        results::ensure(conn)?;
        if first {
            results::clear_session(conn)?;
        }
        Ok(())
    }

    pub fn variable_list(&self, scope: &str) -> Vec<VarInfo> {
        let mut v: Vec<VarInfo> = self
            .variables(scope)
            .into_iter()
            .map(|(name, v)| VarInfo { name, value: v.display, kind: v.ty.name().to_string() })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    /// Make sure schemas exist and stale results are gone (for list/save calls).
    pub fn prepare(&self, scope: &str, conn: &Connection) -> Result<(), String> {
        self.start_session(scope, conn)
    }
}

// ---------------------------------------------------------------- response

#[derive(Serialize, Clone, Debug)]
pub struct VarInfo {
    pub name: String,
    pub value: String,
    pub kind: String,
}

#[derive(Serialize, Clone, Debug)]
pub struct ScalarValue {
    pub var: String,
    pub label: String,
    pub value: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Table { table: String, meta: TableMeta, permanent: bool, label: String },
    Scalars { items: Vec<ScalarValue> },
}

#[derive(Serialize, Clone, Debug)]
pub struct CommandResponse {
    pub command: String,
    pub input: String,
    pub outcome: Outcome,
    pub sql: String,
    pub duration_ms: u64,
    pub log_id: i64,
    pub variables: Vec<VarInfo>,
}

// ---------------------------------------------------------------- run

/// `scope` identifies the folder (its database path); `table` is the input
/// table ("ledger" or "_results.r_3"); `current_row` is where NEXT starts.
pub fn run_command(
    conn: &Connection,
    state: &AnalyticsState,
    scope: &str,
    table: &str,
    text: &str,
    current_row: u64,
) -> CmdResult<CommandResponse> {
    state.start_session(scope, conn)?;
    let t0 = Instant::now();
    let mut sql_for_log = String::new();

    let result = execute(conn, state, scope, table, text, current_row, &mut sql_for_log);
    let ms = t0.elapsed().as_millis() as i64;

    match result {
        Ok((name, outcome, sql)) => {
            let (output, rows) = match &outcome {
                Outcome::Table { table, meta, .. } => (table.clone(), Some(meta.row_count as i64)),
                Outcome::Scalars { items } => (
                    items.iter().map(|i| format!("{} = {}", i.var, i.value.as_deref().unwrap_or("(blank)"))).collect::<Vec<_>>().join(", "),
                    None,
                ),
            };
            let log_id = results::log(
                conn,
                NewLogEntry { command: text.trim(), input: table, ok: true, output: &output, row_count: rows, duration_ms: ms, sql: &sql },
            )
            .unwrap_or(0);
            Ok(CommandResponse {
                command: name.to_string(),
                input: table.to_string(),
                outcome,
                sql,
                duration_ms: ms as u64,
                log_id,
                variables: state.variable_list(scope),
            })
        }
        Err(err) => {
            let _ = results::log(
                conn,
                NewLogEntry { command: text.trim(), input: table, ok: false, output: &err.message, row_count: None, duration_ms: ms, sql: &sql_for_log },
            );
            Err(err)
        }
    }
}

fn execute(
    conn: &Connection,
    state: &AnalyticsState,
    scope: &str,
    table: &str,
    text: &str,
    current_row: u64,
    sql_out: &mut String,
) -> CmdResult<(&'static str, Outcome, String)> {
    // ① lex + ② parse
    let tokens = lexer::tokenize(text)?;
    let mut parser = Parser::new(tokens);
    let command = parse_command(&mut parser)?;

    // ③ bind + ④ plan
    let input = TableRef::parse(table);
    let columns = query::columns(conn, table).map_err(CommandError::general)?;
    let vars = state.variables(scope);
    let ctx = PlanContext {
        binder: Binder::new(&columns, &vars),
        table_sql: input.sql(),
        current_row,
        text,
    };
    let PlannedCommand { plan, output } = command.plan(&ctx)?;

    // ⑤ SQL + ⑥ execute
    match output {
        Output::Scalars(items) => {
            let inner = sqlgen::to_sql(&plan);
            let casts: Vec<String> = items.iter().map(|i| format!("CAST({} AS VARCHAR)", quote_ident(&i.var))).collect();
            let sql = format!("SELECT {} FROM ({inner})", casts.join(", "));
            *sql_out = inner.clone();
            let values: Vec<Option<String>> = conn
                .query_row(&sql, [], |r| (0..items.len()).map(|i| r.get::<_, Option<String>>(i)).collect())
                .map_err(|e| CommandError::general(format!("Command failed: {e}")))?;

            let mut new_vars = Vec::new();
            let mut out = Vec::new();
            for (item, value) in items.iter().zip(values) {
                if let Some(v) = &value {
                    new_vars.push((item.var.clone(), variable(v, item.ty)));
                }
                out.push(ScalarValue { var: item.var.clone(), label: item.label.clone(), value });
            }
            state.set_variables(scope, new_vars);
            Ok((command.name(), Outcome::Scalars { items: out }, inner))
        }

        Output::Table { to, label } => {
            let (target, seq) = match &to {
                Some(t) => {
                    let name = results::validate_new_table_name(conn, &t.name)
                        .map_err(|m| CommandError::new(m, t.span.start, t.span.end))?;
                    (TableRef::main(&name), None)
                }
                None => {
                    let (r, seq) = results::next_result(conn)?;
                    (r, Some(seq))
                }
            };
            let sql = sqlgen::create_table_sql(&plan, &target.sql());
            *sql_out = sql.clone();
            conn.execute_batch(&sql).map_err(|e| CommandError::general(format!("Command failed: {e}")))?;

            let meta = query::table_meta(conn, &target.id())?;
            if let Some(seq) = seq {
                results::register(conn, &target, seq, &label, text.trim(), table, meta.row_count)?;
            }
            Ok((
                command.name(),
                Outcome::Table { table: target.id(), meta, permanent: to.is_some(), label },
                sql,
            ))
        }
    }
}

/// Command output value -> variable usable in later expressions.
fn variable(value: &str, ty: Ty) -> Variable {
    let sql = match ty {
        Ty::Num if value.parse::<f64>().is_ok() => value.to_string(),
        Ty::Date => format!("DATE {}", quote_literal(value)),
        Ty::DateTime => format!("TIMESTAMP {}", quote_literal(value)),
        Ty::Bool => if value == "true" { "TRUE".into() } else { "FALSE".into() },
        _ => quote_literal(value),
    };
    let ty = if ty == Ty::Num && value.parse::<f64>().is_err() { Ty::Text } else { ty };
    Variable { display: value.to_string(), sql, ty }
}
