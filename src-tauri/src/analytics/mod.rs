// src-tauri/src/analytics/mod.rs
//
// ACL command engine.
//
//   text ─▶ dsl::lexer ─▶ dsl::parser + commands::<cmd>::parse ─▶ Command
//        ─▶ binder (fields, types, functions, variables)
//        ─▶ commands::<cmd>::plan ─▶ LogicalPlan (shared nodes)
//        ─▶ sqlgen ─▶ DuckDB SQL ─▶ executor ─▶ scalars | result table
//
// See docs/ACL_COMMANDS.md for the language and docs/ARCHITECTURE.md for design.

pub mod binder;
pub mod commands;
pub mod dsl;
pub mod error;
pub mod executor;
pub mod functions;
pub mod plan;
pub mod results;
pub mod sqlgen;

pub use error::CommandError;
pub use executor::{run_command, AnalyticsState, CommandResponse};
