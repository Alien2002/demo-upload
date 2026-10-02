// src-tauri/src/analytics/commands/summarize.rs
//
//   SUMMARIZE ON key ... [SUBTOTAL num_field ...] [OTHER field ...] [TO "table"]
//             [PRESORT] [IF cond] [WHILE cond] [FIRST n | NEXT n]
//
// One row per distinct key combination, ordered by the keys, with:
//   keys, OTHER fields (value from the group's first record),
//   SUBTOTAL fields (sums), COUNT (records in the group).
// PRESORT is accepted for ACL compatibility; output is always sorted by key.

use super::{base_plan, unique_columns, PlanContext};
use crate::analytics::dsl::ast::{Expr, Scope, Span, Target};
use crate::analytics::dsl::parser::Parser;
use crate::analytics::error::{CmdResult, CommandError};
use crate::analytics::plan::{LogicalPlan, Output, PlannedCommand, RN};
use crate::names::quote_ident;

#[derive(Debug, Clone, PartialEq)]
pub struct SummarizeCommand {
    pub on: Vec<(Expr, Option<String>)>,
    pub subtotal: Vec<(Expr, Option<String>)>,
    pub other: Vec<(Expr, Option<String>)>,
    pub to: Option<Target>,
    pub scope: Scope,
}

pub fn parse(p: &mut Parser) -> CmdResult<SummarizeCommand> {
    p.advance(); // SUMMARIZE
    let mut scope = Scope::default();
    let (mut on, mut subtotal, mut other) = (Vec::new(), Vec::new(), Vec::new());
    let mut to = None;

    while !p.at_end() {
        if p.parse_scope_clause(&mut scope)? {
            continue;
        }
        let span = p.span();
        let list = |p: &mut Parser, target: &mut Vec<(Expr, Option<String>)>, kw: &str, what: &str| -> CmdResult<()> {
            if !target.is_empty() {
                return Err(CommandError::new(format!("{kw} is given twice"), span.start, span.end));
            }
            *target = p.parse_field_list(what)?;
            Ok(())
        };
        if p.eat_keyword("ON") {
            list(p, &mut on, "ON", "fields to group by")?;
        } else if p.eat_keyword("SUBTOTAL") {
            list(p, &mut subtotal, "SUBTOTAL", "number fields to subtotal")?;
        } else if p.eat_keyword("OTHER") {
            list(p, &mut other, "OTHER", "fields to carry over")?;
        } else if p.eat_keyword("TO") {
            if to.is_some() {
                return Err(CommandError::new("TO is given twice", span.start, span.end));
            }
            to = Some(p.parse_target()?);
        } else if p.eat_keyword("PRESORT") {
            // accepted, nothing to do
        } else {
            return Err(p.unexpected("SUMMARIZE accepts ON, SUBTOTAL, OTHER, TO, PRESORT, IF, WHILE, FIRST or NEXT"));
        }
    }
    if on.is_empty() {
        return Err(CommandError::new("SUMMARIZE needs ON and at least one field, e.g. SUMMARIZE ON Account SUBTOTAL Amount", 0, p.span().end));
    }
    Ok(SummarizeCommand { on, subtotal, other, to, scope })
}

pub fn plan(cmd: &SummarizeCommand, ctx: &PlanContext) -> CmdResult<PlannedCommand> {
    let input = base_plan(ctx, &cmd.scope)?;
    let rn = quote_ident(RN);

    let mut group_by = Vec::new();
    let mut names: Vec<(String, String)> = Vec::new();
    let mut spans: Vec<Span> = Vec::new();

    for (expr, alias) in &cmd.on {
        let b = ctx.binder.bind(expr)?;
        let name = ctx.column_name(&b, alias.as_ref());
        group_by.push((b.sql.clone(), name.clone()));
        names.push((b.sql, name));
        spans.push(expr.span.clone());
    }

    let mut aggs = Vec::new();
    for (expr, alias) in &cmd.other {
        let b = ctx.binder.bind(expr)?;
        let name = ctx.column_name(&b, alias.as_ref());
        aggs.push((format!("arg_min({}, {rn})", b.sql), name.clone()));
        names.push((b.sql, name));
        spans.push(expr.span.clone());
    }
    for (expr, alias) in &cmd.subtotal {
        let b = ctx.binder.numeric(expr, "SUBTOTAL")?;
        let name = ctx.column_name(&b, alias.as_ref());
        aggs.push((format!("coalesce(sum({}), 0)", b.sql), name.clone()));
        names.push((b.sql, name));
        spans.push(expr.span.clone());
    }

    // COUNT column, renamed if a field already has that name.
    let mut count_name = "COUNT".to_string();
    let mut n = 2;
    while names.iter().any(|(_, x)| x.eq_ignore_ascii_case(&count_name)) {
        count_name = format!("COUNT_{n}");
        n += 1;
    }
    aggs.push(("count(*)".into(), count_name.clone()));
    names.push(("count(*)".into(), count_name));
    spans.push(Span::new(0, 0));

    unique_columns(&mut names, &spans)?;

    let keys: Vec<String> = cmd.on.iter().map(|(e, _)| ctx.text_of(&e.span)).collect();
    Ok(PlannedCommand {
        plan: LogicalPlan::Aggregate { input: Box::new(input), group_by, aggs },
        output: Output::Table { to: cmd.to.clone(), label: format!("Summarize on {}", keys.join(", ")) },
    })
}
