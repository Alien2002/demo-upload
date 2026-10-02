// src-tauri/src/analytics/sqlgen.rs
//
// LogicalPlan -> DuckDB SQL. The only place SQL is assembled from plans, so a
// fix here (e.g. WHILE semantics) applies to every command at once.

use super::plan::{LogicalPlan, SortKey, RN};
use crate::names::quote_ident;

const OK: &str = "__acl_ok";

pub fn to_sql(plan: &LogicalPlan) -> String {
    let rn = quote_ident(RN);
    match plan {
        LogicalPlan::Scan { table_sql } => format!("SELECT rowid AS {rn}, * FROM {table_sql}"),

        LogicalPlan::Scope { input, start, while_sql, limit } => {
            let mut sql = sub(input);
            if *start > 0 {
                sql = format!("SELECT * FROM {sql} WHERE {rn} >= {start}");
                sql = format!("({sql})");
            }
            if let Some(cond) = while_sql {
                // Running AND of the condition in record order: stays true until the
                // first record where it isn't. A blank (NULL) result also stops, as in ACL.
                let ok = quote_ident(OK);
                sql = format!(
                    "(SELECT * EXCLUDE ({ok}) FROM (\
                     SELECT *, bool_and(coalesce({cond}, false)) OVER \
                     (ORDER BY {rn} ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW) AS {ok} \
                     FROM {sql}) WHERE {ok})"
                );
            }
            match limit {
                Some(n) => format!("SELECT * FROM {sql} ORDER BY {rn} LIMIT {n}"),
                None => format!("SELECT * FROM {sql}"),
            }
        }

        LogicalPlan::Filter { input, predicate } => {
            format!("SELECT * FROM {} WHERE {predicate}", sub(input))
        }

        LogicalPlan::Project { input, items } => {
            let cols: Vec<String> = items.iter().map(|(sql, name)| format!("{sql} AS {}", quote_ident(name))).collect();
            format!("SELECT {rn}, {} FROM {}", cols.join(", "), sub(input))
        }

        LogicalPlan::Sort { input, keys } => {
            format!(
                "SELECT row_number() OVER (ORDER BY {}, {rn}) - 1 AS {rn}, * EXCLUDE ({rn}) FROM {}",
                order_list(keys),
                sub(input)
            )
        }

        LogicalPlan::Aggregate { input, group_by, aggs } => {
            let agg_cols: Vec<String> = aggs.iter().map(|(sql, name)| format!("{sql} AS {}", quote_ident(name))).collect();
            if group_by.is_empty() {
                return format!("SELECT {} FROM {}", agg_cols.join(", "), sub(input));
            }
            let keys: Vec<&str> = group_by.iter().map(|(sql, _)| sql.as_str()).collect();
            let key_cols: Vec<String> = group_by.iter().map(|(sql, name)| format!("{sql} AS {}", quote_ident(name))).collect();
            let order: Vec<String> = keys.iter().map(|k| format!("{k} NULLS LAST")).collect();
            format!(
                "SELECT row_number() OVER (ORDER BY {}) - 1 AS {rn}, {}, {} FROM {} GROUP BY {}",
                order.join(", "),
                key_cols.join(", "),
                agg_cols.join(", "),
                sub(input),
                keys.join(", ")
            )
        }
    }
}

/// Wrap a child plan as a FROM item.
fn sub(plan: &LogicalPlan) -> String {
    match plan {
        LogicalPlan::Scan { table_sql } => format!("(SELECT rowid AS {}, * FROM {table_sql})", quote_ident(RN)),
        other => format!("({})", to_sql(other)),
    }
}

fn order_list(keys: &[SortKey]) -> String {
    keys.iter()
        .map(|k| format!("{} {} NULLS LAST", k.sql, if k.desc { "DESC" } else { "ASC" }))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Final statement that writes a table-producing plan into `target_sql`,
/// keeping record order so the new table's rowids are 0..n-1.
pub fn create_table_sql(plan: &LogicalPlan, target_sql: &str) -> String {
    let rn = quote_ident(RN);
    format!(
        "CREATE TABLE {target_sql} AS SELECT * EXCLUDE ({rn}) FROM ({}) ORDER BY {rn}",
        to_sql(plan)
    )
}
