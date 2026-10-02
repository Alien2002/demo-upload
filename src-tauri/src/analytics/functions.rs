// src-tauri/src/analytics/functions.rs
//
// ACL functions -> DuckDB SQL. Each entry checks its argument count and
// types, then returns the SQL and result type. Add new functions here.

use super::binder::{type_error, BExpr, Ty};
use super::dsl::ast::Span;
use super::error::{CmdResult, CommandError};

/// Names listed in error messages and the command reference.
pub const FUNCTIONS: &[&str] = &[
    "ABS", "ALLTRIM", "BETWEEN", "DAY", "DEC", "FIND", "ISBLANK", "LENGTH", "LOWER", "LTRIM", "MATCH",
    "MAXIMUM", "MINIMUM", "MONTH", "ROUND", "STRING", "SUBSTR", "TRIM", "UPPER", "VALUE", "YEAR",
];

pub fn bind_call(name: &str, args: Vec<BExpr>, span: &Span) -> CmdResult<BExpr> {
    let upper = name.to_ascii_uppercase();
    let f = upper.as_str();
    let ok = |sql: String, ty: Ty| Ok(BExpr { sql, ty, span: span.clone(), field: None });

    let arity = |min: usize, max: usize| -> CmdResult<()> {
        if args.len() < min || args.len() > max {
            let want = if min == max { format!("{min}") } else if max == usize::MAX { format!("{min} or more") } else { format!("{min}–{max}") };
            return Err(CommandError::new(format!("{f}() takes {want} argument(s), got {}", args.len()), span.start, span.end));
        }
        Ok(())
    };
    let want = |i: usize, tys: &[Ty]| -> CmdResult<()> {
        let a = &args[i];
        if tys.contains(&a.ty) {
            Ok(())
        } else {
            let names: Vec<&str> = tys.iter().map(|t| t.name()).collect();
            Err(type_error(&a.span, format!("{f}() argument {} must be {}, not {}", i + 1, names.join(" or "), a.ty.name())))
        }
    };
    let a = |i: usize| args[i].sql.as_str();

    match f {
        // ---- text
        "UPPER" | "LOWER" | "ALLTRIM" | "LTRIM" | "TRIM" => {
            arity(1, 1)?;
            want(0, &[Ty::Text])?;
            let sql_fn = match f {
                "UPPER" => "upper",
                "LOWER" => "lower",
                "ALLTRIM" => "trim",
                "LTRIM" => "ltrim",
                _ => "rtrim", // ACL TRIM removes trailing spaces
            };
            ok(format!("{sql_fn}({})", a(0)), Ty::Text)
        }
        "LENGTH" => {
            arity(1, 1)?;
            want(0, &[Ty::Text])?;
            ok(format!("length({})", a(0)), Ty::Num)
        }
        "SUBSTR" | "SUBSTRING" => {
            arity(3, 3)?;
            want(0, &[Ty::Text])?;
            want(1, &[Ty::Num])?;
            want(2, &[Ty::Num])?;
            ok(format!("substr({}, CAST({} AS BIGINT), CAST({} AS BIGINT))", a(0), a(1), a(2)), Ty::Text)
        }
        // FIND("text") searches the whole record in ACL; here: FIND("text", field). Case-insensitive.
        "FIND" => {
            arity(2, 2)?;
            want(0, &[Ty::Text])?;
            ok(format!("contains(lower(CAST({} AS VARCHAR)), lower({}))", a(1), a(0)), Ty::Bool)
        }
        "ISBLANK" => {
            arity(1, 1)?;
            ok(format!("({0} IS NULL OR trim(CAST({0} AS VARCHAR)) = '')", a(0)), Ty::Bool)
        }
        "STRING" => {
            arity(1, 2)?;
            want(0, &[Ty::Num])?;
            ok(format!("CAST({} AS VARCHAR)", a(0)), Ty::Text)
        }
        "VALUE" => {
            arity(1, 2)?;
            want(0, &[Ty::Text])?;
            if args.len() == 2 {
                want(1, &[Ty::Num])?;
                ok(format!("round(TRY_CAST(replace(trim({}), ',', '') AS DOUBLE), CAST({} AS INTEGER))", a(0), a(1)), Ty::Num)
            } else {
                ok(format!("TRY_CAST(replace(trim({}), ',', '') AS DOUBLE)", a(0)), Ty::Num)
            }
        }

        // ---- numbers
        "ABS" => {
            arity(1, 1)?;
            want(0, &[Ty::Num])?;
            ok(format!("abs({})", a(0)), Ty::Num)
        }
        "ROUND" => {
            arity(1, 1)?;
            want(0, &[Ty::Num])?;
            ok(format!("round({})", a(0)), Ty::Num)
        }
        "DEC" => {
            arity(2, 2)?;
            want(0, &[Ty::Num])?;
            want(1, &[Ty::Num])?;
            ok(format!("round({}, CAST({} AS INTEGER))", a(0), a(1)), Ty::Num)
        }
        "MAXIMUM" | "MINIMUM" => {
            arity(2, 2)?;
            want(0, &[Ty::Num, Ty::Date, Ty::DateTime, Ty::Text])?;
            if !Ty::comparable(args[0].ty, args[1].ty) {
                return Err(type_error(span, format!("{f}() needs two values of the same type")));
            }
            let sql_fn = if f == "MAXIMUM" { "greatest" } else { "least" };
            ok(format!("{sql_fn}({}, {})", a(0), a(1)), args[0].ty)
        }

        // ---- dates
        "YEAR" | "MONTH" | "DAY" => {
            arity(1, 1)?;
            want(0, &[Ty::Date, Ty::DateTime])?;
            ok(format!("{}({})", f.to_ascii_lowercase(), a(0)), Ty::Num)
        }

        // ---- conditions
        "BETWEEN" => {
            arity(3, 3)?;
            for i in 1..3 {
                if !Ty::comparable(args[0].ty, args[i].ty) {
                    return Err(type_error(&args[i].span, format!("BETWEEN() bounds must be {} like the value", args[0].ty.name())));
                }
            }
            ok(format!("({} BETWEEN {} AND {})", a(0), a(1), a(2)), Ty::Bool)
        }
        "MATCH" => {
            arity(2, usize::MAX)?;
            for i in 1..args.len() {
                if !Ty::comparable(args[0].ty, args[i].ty) {
                    return Err(type_error(&args[i].span, format!("MATCH() values must be {} like the first argument", args[0].ty.name())));
                }
            }
            let list: Vec<&str> = (1..args.len()).map(a).collect();
            ok(format!("({} IN ({}))", a(0), list.join(", ")), Ty::Bool)
        }

        _ => Err(CommandError::new(
            format!("Unknown function {f}(). Available: {}", FUNCTIONS.join(", ")),
            span.start,
            span.end,
        )),
    }
}
