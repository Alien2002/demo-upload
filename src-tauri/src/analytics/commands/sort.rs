// src-tauri/src/analytics/commands/sort.rs
//
//   SORT ON key [D] [key [D] ...] [TO "table"]
//        [IF cond] [WHILE cond] [FIRST n | NEXT n]
//
// D after a key sorts it descending. Ties keep their original order.

use super::{base_plan, PlanContext};
use crate::analytics::dsl::ast::{Expr, Scope, Target};
use crate::analytics::dsl::parser::Parser;
use crate::analytics::error::{CmdResult, CommandError};
use crate::analytics::plan::{LogicalPlan, Output, PlannedCommand, SortKey};

#[derive(Debug, Clone, PartialEq)]
pub struct SortCommand {
    pub keys: Vec<(Expr, bool)>,
    pub to: Option<Target>,
    pub scope: Scope,
}

pub fn parse(p: &mut Parser) -> CmdResult<SortCommand> {
    p.advance(); // SORT
    let mut scope = Scope::default();
    let mut keys: Vec<(Expr, bool)> = Vec::new();
    let mut to = None;

    while !p.at_end() {
        if p.parse_scope_clause(&mut scope)? {
            continue;
        }
        let span = p.span();
        if p.eat_keyword("ON") {
            if !keys.is_empty() {
                return Err(CommandError::new("ON is given twice; list all keys after one ON", span.start, span.end));
            }
            while !p.at_end() && !p.at_any_clause_keyword() {
                let key = p.parse_expr()?;
                let desc = p.eat_keyword("D");
                keys.push((key, desc));
            }
            if keys.is_empty() {
                return Err(p.unexpected("ON needs at least one field to sort by"));
            }
            continue;
        }
        if p.eat_keyword("TO") {
            if to.is_some() {
                return Err(CommandError::new("TO is given twice", span.start, span.end));
            }
            to = Some(p.parse_target()?);
            continue;
        }
        return Err(p.unexpected("SORT accepts ON, TO, IF, WHILE, FIRST or NEXT"));
    }
    if keys.is_empty() {
        return Err(CommandError::new("SORT needs ON and at least one field, e.g. SORT ON Amount D", 0, p.span().end));
    }
    Ok(SortCommand { keys, to, scope })
}

pub fn plan(cmd: &SortCommand, ctx: &PlanContext) -> CmdResult<PlannedCommand> {
    let input = base_plan(ctx, &cmd.scope)?;
    let mut keys = Vec::new();
    let mut label = Vec::new();
    for (expr, desc) in &cmd.keys {
        let b = ctx.binder.bind(expr)?;
        keys.push(SortKey { sql: b.sql, desc: *desc });
        label.push(format!("{}{}", ctx.text_of(&expr.span), if *desc { " D" } else { "" }));
    }
    Ok(PlannedCommand {
        plan: LogicalPlan::Sort { input: Box::new(input), keys },
        output: Output::Table { to: cmd.to.clone(), label: format!("Sort on {}", label.join(", ")) },
    })
}
