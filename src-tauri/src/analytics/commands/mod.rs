// src-tauri/src/analytics/commands/mod.rs
//
// Command registry. To add a command:
//   1. new file here with its AST struct, `parse` and `plan`
//   2. a variant in `Command`
//   3. one arm in `parse_command` and one in `Command::plan`
// The compiler then points at anything else that must handle it.

pub mod count;
pub mod extract;
pub mod sort;
pub mod summarize;
pub mod total;

use super::binder::{BExpr, Binder};
use super::dsl::ast::{FirstNextKind, Scope, Span};
use super::dsl::parser::Parser;
use super::dsl::token::Token;
use super::error::{CmdResult, CommandError};
use super::plan::{LogicalPlan, PlannedCommand};

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Count(count::CountCommand),
    Total(total::TotalCommand),
    Extract(extract::ExtractCommand),
    Sort(sort::SortCommand),
    Summarize(summarize::SummarizeCommand),
}

/// Command names (and accepted spellings) for messages and help.
pub const COMMANDS: &[&str] = &["COUNT", "EXTRACT", "SORT", "SUMMARIZE", "TOTAL"];

pub fn parse_command(p: &mut Parser) -> CmdResult<Command> {
    let word = match &p.current().token {
        Token::Ident(w) => w.to_ascii_uppercase(),
        Token::Eof => return Err(p.error_here("Type a command, e.g. COUNT IF Amount > 1000")),
        _ => return Err(p.unexpected(&format!("a command must start with one of: {}", COMMANDS.join(", ")))),
    };
    let cmd = match word.as_str() {
        "COUNT" => Command::Count(count::parse(p)?),
        "TOTAL" => Command::Total(total::parse(p)?),
        "EXTRACT" => Command::Extract(extract::parse(p)?),
        "SORT" => Command::Sort(sort::parse(p)?),
        "SUMMARIZE" | "SUMMARISE" => Command::Summarize(summarize::parse(p)?),
        _ => {
            return Err(p.error_here(format!(
                "Unknown command '{word}'. Available: {}",
                COMMANDS.join(", ")
            )))
        }
    };
    p.expect_end()?;
    Ok(cmd)
}

impl Command {
    pub fn name(&self) -> &'static str {
        match self {
            Command::Count(_) => "COUNT",
            Command::Total(_) => "TOTAL",
            Command::Extract(_) => "EXTRACT",
            Command::Sort(_) => "SORT",
            Command::Summarize(_) => "SUMMARIZE",
        }
    }

    pub fn plan(&self, ctx: &PlanContext) -> CmdResult<PlannedCommand> {
        match self {
            Command::Count(c) => count::plan(c, ctx),
            Command::Total(c) => total::plan(c, ctx),
            Command::Extract(c) => extract::plan(c, ctx),
            Command::Sort(c) => sort::plan(c, ctx),
            Command::Summarize(c) => summarize::plan(c, ctx),
        }
    }
}

/// Everything a planner needs.
pub struct PlanContext<'a> {
    pub binder: Binder<'a>,
    /// Quoted SQL reference of the input table.
    pub table_sql: String,
    /// Record number of the selected row (for NEXT).
    pub current_row: u64,
    /// Original command text (for labels and default column names).
    pub text: &'a str,
}

impl PlanContext<'_> {
    /// The command text a span covers.
    pub fn text_of(&self, span: &Span) -> String {
        self.text.chars().skip(span.start).take(span.end.saturating_sub(span.start)).collect::<String>().trim().to_string()
    }

    /// Display name for an output column: the field name, or the expression text.
    pub fn column_name(&self, b: &BExpr, alias: Option<&String>) -> String {
        alias.cloned().or_else(|| b.field.clone()).unwrap_or_else(|| self.text_of(&b.span))
    }
}

/// The part every command shares: Scan -> Scope (FIRST/NEXT/WHILE) -> Filter (IF).
/// ACL semantics: the scope decides which records are processed; IF then
/// selects among those.
pub fn base_plan(ctx: &PlanContext, scope: &Scope) -> CmdResult<LogicalPlan> {
    let mut plan = LogicalPlan::Scan { table_sql: ctx.table_sql.clone() };

    let while_sql = match &scope.while_ {
        Some(e) => Some(ctx.binder.condition(e, "WHILE")?.sql),
        None => None,
    };
    let (start, limit) = match &scope.first_next {
        Some(f) if f.kind == FirstNextKind::Next => (ctx.current_row, Some(f.n)),
        Some(f) => (0, Some(f.n)),
        None => (0, None),
    };
    if while_sql.is_some() || limit.is_some() || start > 0 {
        plan = LogicalPlan::Scope { input: Box::new(plan), start, while_sql, limit };
    }

    if let Some(e) = &scope.if_ {
        let pred = ctx.binder.condition(e, "IF")?;
        plan = LogicalPlan::Filter { input: Box::new(plan), predicate: pred.sql };
    }
    Ok(plan)
}

/// Column names must be unique in a table (case-insensitive).
pub fn unique_columns(names: &mut [(String, String)], spans: &[Span]) -> CmdResult<()> {
    for i in 0..names.len() {
        for j in 0..i {
            if names[i].1.eq_ignore_ascii_case(&names[j].1) {
                let s = spans.get(i).cloned().unwrap_or(Span::new(0, 0));
                return Err(CommandError::new(
                    format!("Two output columns are named '{}'; rename one with AS \"…\"", names[i].1),
                    s.start,
                    s.end,
                ));
            }
        }
    }
    Ok(())
}
