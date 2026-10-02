// src-tauri/src/analytics/plan.rs
//
// The ONE plan type every command builds. Commands differ only in how they
// stack these nodes; SQL for all of them comes from sqlgen.rs.
//
// Record order: ACL processes records in file order. Every plan carries a
// hidden record-number column (RN) from the scan up, so FIRST/NEXT/WHILE and
// the final table keep the right order even through subqueries.

use super::binder::Ty;

/// Hidden record-number column carried through every plan.
pub const RN: &str = "__acl_rn";

#[derive(Debug, Clone, PartialEq)]
pub enum LogicalPlan {
    /// Read a table in record order. `table_sql` is already quoted.
    Scan { table_sql: String },

    /// FIRST/NEXT/WHILE: which records are processed.
    Scope {
        input: Box<LogicalPlan>,
        /// First record number to process (NEXT starts at the current row).
        start: u64,
        /// Stop at the first record where this is not true.
        while_sql: Option<String>,
        /// Process at most this many records.
        limit: Option<u64>,
    },

    /// IF: keep records where the condition is true.
    Filter { input: Box<LogicalPlan>, predicate: String },

    /// Choose / compute output columns: (sql, column name).
    Project { input: Box<LogicalPlan>, items: Vec<(String, String)> },

    /// Reorder records; record numbers are renumbered in the new order.
    Sort { input: Box<LogicalPlan>, keys: Vec<SortKey> },

    /// Group and aggregate. With no group keys it returns one row (scalars)
    /// and no record number.
    Aggregate {
        input: Box<LogicalPlan>,
        group_by: Vec<(String, String)>,
        aggs: Vec<(String, String)>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct SortKey {
    pub sql: String,
    pub desc: bool,
}

/// What a command produces.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// One row of named values; each also becomes a variable (COUNT1, TOTAL1…).
    Scalars(Vec<ScalarItem>),
    /// A table: permanent if `to` is given, otherwise a session result.
    Table { to: Option<super::dsl::ast::Target>, label: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ScalarItem {
    /// Column alias in the plan == variable name.
    pub var: String,
    /// "Count", "Total of Amount"…
    pub label: String,
    pub ty: Ty,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedCommand {
    pub plan: LogicalPlan,
    pub output: Output,
}
