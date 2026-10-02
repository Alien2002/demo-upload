// src-tauri/src/analytics/commands/extract.rs
//
//   EXTRACT [RECORD | FIELDS item [AS "name"] ...] [TO "table"]
//           [IF cond] [WHILE cond] [FIRST n | NEXT n]
//
// Items can be fields or expressions (Amount * 1.18 AS "Gross").
// Without TO the result is a session table you can keep querying.

use super::{base_plan, unique_columns, PlanContext};
use crate::analytics::dsl::ast::{Expr, Scope, Span, Target};
use crate::analytics::dsl::parser::Parser;
use crate::analytics::error::{CmdResult, CommandError};
use crate::analytics::plan::{LogicalPlan, Output, PlannedCommand};

#[derive(Debug, Clone, PartialEq)]
pub enum FieldSelection {
    Record,
    Fields(Vec<(Expr, Option<String>)>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ExtractCommand {
    pub fields: FieldSelection,
    pub to: Option<Target>,
    pub scope: Scope,
}

pub fn parse(p: &mut Parser) -> CmdResult<ExtractCommand> {
    p.advance(); // EXTRACT
    let mut scope = Scope::default();
    let mut fields: Option<FieldSelection> = None;
    let mut to = None;

    while !p.at_end() {
        if p.parse_scope_clause(&mut scope)? {
            continue;
        }
        let span = p.span();
        if p.eat_keyword("RECORD") {
            if fields.is_some() {
                return Err(CommandError::new("Use either RECORD or FIELDS, once", span.start, span.end));
            }
            fields = Some(FieldSelection::Record);
            continue;
        }
        if p.eat_keyword("FIELDS") {
            if fields.is_some() {
                return Err(CommandError::new("Use either RECORD or FIELDS, once", span.start, span.end));
            }
            fields = Some(FieldSelection::Fields(p.parse_field_list("fields to extract")?));
            continue;
        }
        if p.eat_keyword("TO") {
            if to.is_some() {
                return Err(CommandError::new("TO is given twice", span.start, span.end));
            }
            to = Some(p.parse_target()?);
            continue;
        }
        return Err(p.unexpected("EXTRACT accepts RECORD, FIELDS, TO, IF, WHILE, FIRST or NEXT"));
    }

    Ok(ExtractCommand { fields: fields.unwrap_or(FieldSelection::Record), to, scope })
}

pub fn plan(cmd: &ExtractCommand, ctx: &PlanContext) -> CmdResult<PlannedCommand> {
    let mut plan = base_plan(ctx, &cmd.scope)?;

    if let FieldSelection::Fields(items) = &cmd.fields {
        let mut cols: Vec<(String, String)> = Vec::new();
        let mut spans: Vec<Span> = Vec::new();
        for (expr, alias) in items {
            let b = ctx.binder.bind(expr)?;
            cols.push((b.sql.clone(), ctx.column_name(&b, alias.as_ref())));
            spans.push(expr.span.clone());
        }
        unique_columns(&mut cols, &spans)?;
        plan = LogicalPlan::Project { input: Box::new(plan), items: cols };
    }

    let what = match &cmd.fields {
        FieldSelection::Record => "Extract".to_string(),
        FieldSelection::Fields(f) => format!("Extract {} field(s)", f.len()),
    };
    let cond = cmd.scope.if_.as_ref().map(|e| format!(" IF {}", ctx.text_of(&e.span))).unwrap_or_default();
    Ok(PlannedCommand { plan, output: Output::Table { to: cmd.to.clone(), label: format!("{what}{cond}") } })
}
