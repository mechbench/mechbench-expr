//! An expression's canonical form (one spacing and quoting, so `a==1`
//! and `a == 1` hash alike) and the field paths it reads (for checking
//! against the upstream kind before anything runs).

use crate::ast::{BinOp, CmpOp, CompKind, Expr, ExprKind, UnOp};
use crate::value::py_repr;
use serde_json::Value;
use std::collections::BTreeSet;

/// Binding strength, Python's order: higher binds tighter.
fn prec(e: &Expr) -> u8 {
    match &e.kind {
        ExprKind::IfElse { .. } => 1,
        ExprKind::Or(..) => 2,
        ExprKind::And(..) => 3,
        ExprKind::Unary(UnOp::Not, _) => 4,
        ExprKind::Compare(..) => 5,
        ExprKind::Binary(BinOp::Add | BinOp::Sub, ..) => 6,
        ExprKind::Binary(BinOp::Mul | BinOp::Div | BinOp::FloorDiv | BinOp::Mod, ..) => 7,
        ExprKind::Unary(..) => 8,
        ExprKind::Binary(BinOp::Pow, ..) => 9,
        ExprKind::Comp {
            kind: CompKind::Generator,
            ..
        } => 0,
        _ => 10,
    }
}

pub fn canonical(e: &Expr) -> String {
    print(e)
}

fn wrap(e: &Expr, min: u8) -> String {
    let s = print(e);
    if prec(e) < min { format!("({s})") } else { s }
}

fn lit(v: &Value) -> String {
    match v {
        Value::String(s) => serde_json::to_string(s).unwrap_or_default(),
        other => py_repr(other),
    }
}

fn args(a: &[crate::ast::Arg]) -> String {
    a.iter()
        .map(|x| match &x.name {
            Some(n) => format!("{n}={}", print(&x.value)),
            None => print(&x.value),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn print(e: &Expr) -> String {
    match &e.kind {
        ExprKind::Lit(v) => lit(v),
        ExprKind::Name(n) => n.clone(),
        ExprKind::Attr(t, f) => format!("{}.{f}", wrap(t, 10)),
        ExprKind::Index(t, i) => format!("{}[{}]", wrap(t, 10), print(i)),
        ExprKind::Slice(t, a, b, c) => {
            let p = |x: &Option<Box<Expr>>| x.as_ref().map_or(String::new(), |x| print(x));
            let step = c
                .as_ref()
                .map_or(String::new(), |c| format!(":{}", print(c)));
            format!("{}[{}:{}{}]", wrap(t, 10), p(a), p(b), step)
        }
        ExprKind::Call(f, a) => format!("{f}({})", args(a)),
        ExprKind::Method(t, m, a) => format!("{}.{m}({})", wrap(t, 10), args(a)),
        ExprKind::Unary(UnOp::Not, x) => format!("not {}", wrap(x, 4)),
        ExprKind::Unary(op, x) => {
            format!("{}{}", if *op == UnOp::Neg { "-" } else { "+" }, wrap(x, 8))
        }
        ExprKind::Binary(op, l, r) => {
            let (sym, p) = match op {
                BinOp::Add => ("+", 6),
                BinOp::Sub => ("-", 6),
                BinOp::Mul => ("*", 7),
                BinOp::Div => ("/", 7),
                BinOp::FloorDiv => ("//", 7),
                BinOp::Mod => ("%", 7),
                BinOp::Pow => ("**", 9),
            };
            // Left-associative, except `**`.
            let (lp, rp) = if *op == BinOp::Pow {
                (p + 1, p)
            } else {
                (p, p + 1)
            };
            let right = if *op == BinOp::Pow {
                wrap(r, 8)
            } else {
                wrap(r, rp)
            };
            format!("{} {sym} {}", wrap(l, lp), right)
        }
        ExprKind::Compare(first, rest) => {
            let mut s = wrap(first, 6);
            for (op, x) in rest {
                let sym = match op {
                    CmpOp::Eq => "==",
                    CmpOp::Ne => "!=",
                    CmpOp::Lt => "<",
                    CmpOp::Le => "<=",
                    CmpOp::Gt => ">",
                    CmpOp::Ge => ">=",
                    CmpOp::In => "in",
                    CmpOp::NotIn => "not in",
                    CmpOp::Is => "is",
                    CmpOp::IsNot => "is not",
                };
                s = format!("{s} {sym} {}", wrap(x, 6));
            }
            s
        }
        ExprKind::And(l, r) => format!("{} and {}", wrap(l, 3), wrap(r, 4)),
        ExprKind::Or(l, r) => format!("{} or {}", wrap(l, 2), wrap(r, 3)),
        ExprKind::IfElse {
            then,
            cond,
            otherwise,
        } => format!(
            "{} if {} else {}",
            wrap(then, 2),
            wrap(cond, 2),
            wrap(otherwise, 1)
        ),
        ExprKind::List(xs) => format!("[{}]", xs.iter().map(print).collect::<Vec<_>>().join(", ")),
        ExprKind::Dict(ps) => format!(
            "{{{}}}",
            ps.iter()
                .map(|(k, v)| format!("{}: {}", print(k), print(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        ExprKind::Comp {
            kind,
            elt,
            var,
            iter,
            cond,
        } => {
            let c = cond
                .as_ref()
                .map_or(String::new(), |c| format!(" if {}", wrap(c, 2)));
            let body = format!("{} for {var} in {}{c}", print(elt), wrap(iter, 2));
            if *kind == CompKind::List {
                format!("[{body}]")
            } else {
                body
            }
        }
    }
}

/// The field paths an expression reads from the record, as dot paths with
/// `[]` for "each element": `tracked.truth.token`, `top[].token.text`,
/// `votes[].winner`. Reads through `params` and `header` are prefixed
/// with those roots.
pub fn reads(e: &Expr) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut locals: Vec<(String, Option<String>)> = Vec::new();
    walk(e, &mut locals, &mut out);
    out
}

/// The path an expression names, when it is a chain of fields and
/// indexes from a root or a comprehension variable.
fn path(e: &Expr, locals: &[(String, Option<String>)]) -> Option<String> {
    match &e.kind {
        ExprKind::Name(n) => match locals.iter().rev().find(|(k, _)| k == n) {
            Some((_, p)) => p.clone(),
            None if n == "record" => Some(String::new()),
            None => Some(n.clone()),
        },
        ExprKind::Attr(t, f) => path(t, locals).map(|p| {
            if p.is_empty() {
                f.clone()
            } else {
                format!("{p}.{f}")
            }
        }),
        ExprKind::Index(t, i) => {
            let p = path(t, locals)?;
            match &i.kind {
                ExprKind::Lit(Value::String(k)) => Some(if p.is_empty() {
                    k.clone()
                } else {
                    format!("{p}.{k}")
                }),
                _ => Some(format!("{p}[]")),
            }
        }
        _ => None,
    }
}

fn walk(e: &Expr, locals: &mut Vec<(String, Option<String>)>, out: &mut BTreeSet<String>) {
    if let Some(p) = path(e, locals) {
        if !p.is_empty() {
            out.insert(p);
        }
        // The index expression of a subscript is read too: `xs[i]`.
        if let ExprKind::Index(_, i) = &e.kind {
            walk(i, locals, out);
        }
        return;
    }
    match &e.kind {
        ExprKind::Lit(_) | ExprKind::Name(_) => {}
        ExprKind::Attr(t, _) => walk(t, locals, out),
        ExprKind::Index(t, i) => {
            walk(t, locals, out);
            walk(i, locals, out);
        }
        ExprKind::Slice(t, a, b, c) => {
            walk(t, locals, out);
            for x in [a, b, c].into_iter().flatten() {
                walk(x, locals, out);
            }
        }
        ExprKind::Call(_, a) => a.iter().for_each(|x| walk(&x.value, locals, out)),
        ExprKind::Method(t, _, a) => {
            walk(t, locals, out);
            a.iter().for_each(|x| walk(&x.value, locals, out));
        }
        ExprKind::Unary(_, x) => walk(x, locals, out),
        ExprKind::Binary(_, l, r) | ExprKind::And(l, r) | ExprKind::Or(l, r) => {
            walk(l, locals, out);
            walk(r, locals, out);
        }
        ExprKind::Compare(f, rest) => {
            walk(f, locals, out);
            rest.iter().for_each(|(_, x)| walk(x, locals, out));
        }
        ExprKind::IfElse {
            then,
            cond,
            otherwise,
        } => {
            walk(then, locals, out);
            walk(cond, locals, out);
            walk(otherwise, locals, out);
        }
        ExprKind::List(xs) => xs.iter().for_each(|x| walk(x, locals, out)),
        ExprKind::Dict(ps) => ps.iter().for_each(|(k, v)| {
            walk(k, locals, out);
            walk(v, locals, out);
        }),
        ExprKind::Comp {
            elt,
            var,
            iter,
            cond,
            ..
        } => {
            walk(iter, locals, out);
            let each = path(iter, locals).map(|p| format!("{p}[]"));
            locals.push((var.clone(), each));
            walk(elt, locals, out);
            if let Some(c) = cond {
                walk(c, locals, out);
            }
            locals.pop();
        }
    }
}
