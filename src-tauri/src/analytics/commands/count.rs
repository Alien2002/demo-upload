// src-tauri/src/analytics/commands/count.rs
//
//   COUNT [IF cond] [WHILE cond] [FIRST n | NEXT n]
//
// Result: COUNT1 (number of records).

use super::{base_plan, PlanContext};
use crate::analytics::binder::Ty;
use crate::analytics::dsl::ast::Scope;
use crate::analytics::dsl::parser::Parser;
use crate::analytics::error::CmdResult;
use crate::analytics::plan::{LogicalPlan, Output, PlannedCommand, ScalarItem};

#[derive(Debug, Clone, PartialEq)]
pub struct CountCommand {
    pub scope: Scope,
}

pub fn parse(p: &mut Parser) -> CmdResult<CountCommand> {
    p.advance(); // COUNT
    let mut scope = Scope::default();
    while !p.at_end() {
        if p.parse_scope_clause(&mut scope)? {
            continue;
        }
        return Err(p.unexpected("COUNT accepts IF, WHILE, FIRST or NEXT"));
    }
    Ok(CountCommand { scope })
}

pub fn plan(cmd: &CountCommand, ctx: &PlanContext) -> CmdResult<PlannedCommand> {
    let input = base_plan(ctx, &cmd.scope)?;
    Ok(PlannedCommand {
        plan: LogicalPlan::Aggregate {
            input: Box::new(input),
            group_by: vec![],
            aggs: vec![("count(*)".into(), "COUNT1".into())],
        },
        output: Output::Scalars(vec![ScalarItem { var: "COUNT1".into(), label: "Count".into(), ty: Ty::Num }]),
    })
}
