// src-tauri/src/analytics/commands/total.rs
//
//   TOTAL [FIELDS] num_field|expr ... [IF cond] [WHILE cond] [FIRST n | NEXT n]
//
// Results: TOTAL1, TOTAL2, … one per field, in order.

use super::{base_plan, PlanContext};
use crate::analytics::binder::Ty;
use crate::analytics::dsl::ast::{Expr, Scope};
use crate::analytics::dsl::parser::Parser;
use crate::analytics::error::{CmdResult, CommandError};
use crate::analytics::plan::{LogicalPlan, Output, PlannedCommand, ScalarItem};

#[derive(Debug, Clone, PartialEq)]
pub struct TotalCommand {
    pub fields: Vec<Expr>,
    pub scope: Scope,
}

pub fn parse(p: &mut Parser) -> CmdResult<TotalCommand> {
    p.advance(); // TOTAL
    let mut scope = Scope::default();
    let mut fields = Vec::new();
    while !p.at_end() {
        if p.parse_scope_clause(&mut scope)? {
            continue;
        }
        if p.eat_keyword("FIELDS") || (fields.is_empty() && !p.at_any_clause_keyword()) {
            if !fields.is_empty() {
                return Err(p.error_here("FIELDS is given twice"));
            }
            for (expr, alias) in p.parse_field_list("number fields to total")? {
                if alias.is_some() {
                    return Err(CommandError::new("TOTAL doesn't take AS names", expr.span.start, expr.span.end));
                }
                fields.push(expr);
            }
            continue;
        }
        return Err(p.unexpected("TOTAL accepts FIELDS, IF, WHILE, FIRST or NEXT"));
    }
    if fields.is_empty() {
        return Err(CommandError::new("TOTAL needs at least one number field, e.g. TOTAL Amount", 0, p.span().end));
    }
    Ok(TotalCommand { fields, scope })
}

pub fn plan(cmd: &TotalCommand, ctx: &PlanContext) -> CmdResult<PlannedCommand> {
    let input = base_plan(ctx, &cmd.scope)?;
    let mut aggs = Vec::new();
    let mut items = Vec::new();
    for (i, f) in cmd.fields.iter().enumerate() {
        let b = ctx.binder.numeric(f, "TOTAL")?;
        let var = format!("TOTAL{}", i + 1);
        aggs.push((format!("coalesce(sum({}), 0)", b.sql), var.clone()));
        items.push(ScalarItem { var, label: format!("Total of {}", ctx.text_of(&f.span)), ty: Ty::Num });
    }
    Ok(PlannedCommand {
        plan: LogicalPlan::Aggregate { input: Box::new(input), group_by: vec![], aggs },
        output: Output::Scalars(items),
    })
}
