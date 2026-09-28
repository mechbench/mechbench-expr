//! The fixed library: functions and methods, each total and the same on
//! every host. Null in gives null out, unless a function is about null
//! (`coalesce`, `len` of a list that holds some).

use crate::error::{Error, Span};
use crate::eval::{Ctx, cond};
use crate::parser::float;
use crate::pyfmt::{format_number, fsum, round_to};
use crate::value::{Num, cmp, eq, int, num, py_str, type_name};
use serde_json::Value;
use std::cmp::Ordering;

/// The functions the language has, for messages and for `check`.
pub const FUNCTIONS: &[&str] = &[
    "abs", "round", "floor", "ceil", "int", "float", "str", "min", "max", "sqrt", "exp", "log",
    "log2", "log10", "pow", "len", "sum", "any", "all", "sorted", "first", "last", "count",
    "contains", "coalesce", "keys", "values", "has", "format",
];

struct Args<'a> {
    f: &'a str,
    pos: Vec<Value>,
    named: Vec<(String, Value)>,
    span: Span,
}

impl<'a> Args<'a> {
    fn arity(&self, min: usize, max: usize) -> Result<(), Error> {
        let n = self.pos.len();
        if n < min || n > max {
            let want = if min == max {
                format!("{min}")
            } else {
                format!("{min} to {max}")
            };
            return Err(Error::type_(
                format!(
                    "{}() takes {want} argument{}, not {n}",
                    self.f,
                    if max == 1 { "" } else { "s" }
                ),
                self.span,
            ));
        }
        Ok(())
    }
    fn get(&self, i: usize, name: &str) -> Option<&Value> {
        self.named
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
            .or_else(|| self.pos.get(i))
    }
    fn check_names(&self, allowed: &[&str]) -> Result<(), Error> {
        for (k, _) in &self.named {
            if !allowed.contains(&k.as_str()) {
                return Err(Error::type_(
                    format!("{}() takes no argument named `{k}`", self.f),
                    self.span,
                ));
            }
        }
        Ok(())
    }
    fn number(&self, v: &Value) -> Result<Option<Num>, Error> {
        match v {
            Value::Null => Ok(None),
            _ => num(v).map(Some).ok_or_else(|| {
                Error::type_(
                    format!("{}() takes a number, not {}", self.f, type_name(v)),
                    self.span,
                )
            }),
        }
    }
    fn list(&self, v: &'a Value) -> Result<Option<&'a Vec<Value>>, Error> {
        match v {
            Value::Null => Ok(None),
            Value::Array(xs) => Ok(Some(xs)),
            other => Err(Error::type_(
                format!("{}() takes a list, not {}", self.f, type_name(other)),
                self.span,
            )),
        }
    }
    fn string(&self, v: &'a Value) -> Result<Option<&'a str>, Error> {
        match v {
            Value::Null => Ok(None),
            Value::String(s) => Ok(Some(s)),
            other => Err(Error::type_(
                format!("{}() takes a string, not {}", self.f, type_name(other)),
                self.span,
            )),
        }
    }
}

pub fn call(
    f: &str,
    pos: Vec<Value>,
    named: Vec<(String, Value)>,
    span: Span,
    ctx: &mut Ctx,
) -> Result<Value, Error> {
    let a = Args {
        f,
        pos,
        named,
        span,
    };
    match f {
        "abs" => {
            a.arity(1, 1)?;
            Ok(match a.number(&a.pos[0])? {
                None => Value::Null,
                Some(Num::I(i)) => int(i
                    .checked_abs()
                    .ok_or_else(|| Error::range("the integer is too large", span))?),
                Some(Num::F(x)) => float(x.abs()),
            })
        }
        "round" => {
            a.check_names(&["ndigits"])?;
            if a.pos.is_empty() || a.pos.len() > 2 {
                a.arity(1, 2)?;
            }
            let digits = a.get(1, "ndigits").cloned().unwrap_or(Value::Null);
            let x = match a.number(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(x) => x,
            };
            match (x, &digits) {
                (Num::I(i), Value::Null) => Ok(int(i)),
                (Num::F(v), Value::Null) => {
                    let r = round_to(v, 0);
                    if r.abs() >= 9.2e18 {
                        return Err(Error::range(
                            "the rounded value is larger than an integer holds",
                            span,
                        ));
                    }
                    Ok(int(r as i64))
                }
                (_, d) => {
                    let n = match num(d) {
                        Some(Num::I(n)) if (0..=17).contains(&n) => n as u32,
                        Some(Num::I(_)) => {
                            return Err(Error::range("round() takes 0 to 17 decimal places", span));
                        }
                        _ => return Err(Error::type_("round()'s ndigits is an integer", span)),
                    };
                    Ok(match x {
                        Num::I(i) => int(i),
                        Num::F(v) => float(round_to(v, n)),
                    })
                }
            }
        }
        "floor" | "ceil" => {
            a.arity(1, 1)?;
            Ok(match a.number(&a.pos[0])? {
                None => Value::Null,
                Some(Num::I(i)) => int(i),
                Some(Num::F(x)) => {
                    let r = if f == "floor" {
                        libm::floor(x)
                    } else {
                        libm::ceil(x)
                    };
                    if r.abs() >= 9.2e18 {
                        return Err(Error::range(
                            "the value is larger than an integer holds",
                            span,
                        ));
                    }
                    int(r as i64)
                }
            })
        }
        "int" => {
            a.arity(1, 1)?;
            match &a.pos[0] {
                Value::Null => Ok(Value::Null),
                Value::Bool(b) => Ok(int(*b as i64)),
                Value::String(s) => {
                    s.trim()
                        .replace('_', "")
                        .parse::<i64>()
                        .map(int)
                        .map_err(|_| {
                            Error::type_(format!("int() cannot read {s:?} as an integer"), span)
                        })
                }
                v => match num(v) {
                    Some(Num::I(i)) => Ok(int(i)),
                    Some(Num::F(x)) => {
                        let t = x.trunc();
                        if t.abs() >= 9.2e18 {
                            return Err(Error::range(
                                "the value is larger than an integer holds",
                                span,
                            ));
                        }
                        Ok(int(t as i64))
                    }
                    None => Err(Error::type_(
                        format!("int() cannot read {}", type_name(v)),
                        span,
                    )),
                },
            }
        }
        "float" => {
            a.arity(1, 1)?;
            match &a.pos[0] {
                Value::Null => Ok(Value::Null),
                Value::Bool(b) => Ok(float(if *b { 1.0 } else { 0.0 })),
                Value::String(s) => match s.trim().parse::<f64>() {
                    Ok(x) if x.is_finite() => Ok(float(x)),
                    _ => Err(Error::type_(
                        format!("float() cannot read {s:?} as a number"),
                        span,
                    )),
                },
                v => num(v).map(|n| float(n.f())).ok_or_else(|| {
                    Error::type_(format!("float() cannot read {}", type_name(v)), span)
                }),
            }
        }
        "str" => {
            a.arity(1, 1)?;
            Ok(Value::String(py_str(&a.pos[0])))
        }
        "min" | "max" => {
            let items: Vec<Value> = if a.pos.len() == 1 {
                match a.list(&a.pos[0])? {
                    None => return Ok(Value::Null),
                    Some(xs) => xs.clone(),
                }
            } else {
                a.pos.clone()
            };
            if items.is_empty() {
                return Err(Error::range(format!("{f}() of an empty list"), span));
            }
            if items.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let mut best = items[0].clone();
            for x in &items[1..] {
                let o = cmp(x, &best).map_err(|m| Error::type_(m, span))?;
                let better = if f == "min" {
                    o == Some(Ordering::Less)
                } else {
                    o == Some(Ordering::Greater)
                };
                if better {
                    best = x.clone();
                }
            }
            Ok(best)
        }
        "sqrt" | "exp" | "log2" | "log10" => {
            a.arity(1, 1)?;
            let x = match a.number(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(x) => x.f(),
            };
            let (r, why) = match f {
                "sqrt" => (libm::sqrt(x), "the square root of a negative number"),
                "exp" => (libm::exp(x), "a result too large for a number"),
                "log2" => (libm::log2(x), "the log of zero or a negative number"),
                _ => (libm::log10(x), "the log of zero or a negative number"),
            };
            Ok(ctx.float(r, why))
        }
        "log" => {
            a.check_names(&["base"])?;
            if a.pos.is_empty() || a.pos.len() > 2 {
                a.arity(1, 2)?;
            }
            let x = match a.number(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(x) => x.f(),
            };
            let base = match a.get(1, "base") {
                None => None,
                Some(b) => match a.number(b)? {
                    None => return Ok(Value::Null),
                    Some(b) => Some(b.f()),
                },
            };
            let r = match base {
                None => libm::log(x),
                Some(b) => libm::log(x) / libm::log(b),
            };
            Ok(ctx.float(r, "the log of zero or a negative number"))
        }
        "pow" => {
            a.arity(2, 2)?;
            crate::eval::binary(crate::ast::BinOp::Pow, &a.pos[0], &a.pos[1], span, ctx)
        }
        "len" => {
            a.arity(1, 1)?;
            match &a.pos[0] {
                Value::Null => Ok(Value::Null),
                Value::String(s) => Ok(int(s.chars().count() as i64)),
                Value::Array(xs) => Ok(int(xs.len() as i64)),
                Value::Object(m) => Ok(int(m.len() as i64)),
                other => Err(Error::type_(format!("len() of {}", type_name(other)), span)),
            }
        }
        "sum" => {
            a.arity(1, 1)?;
            let xs = match a.list(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(xs) => xs,
            };
            if xs.iter().any(Value::is_null) {
                return Ok(Value::Null);
            }
            let nums: Vec<Num> = xs
                .iter()
                .map(|x| a.number(x).map(|n| n.unwrap_or(Num::I(0))))
                .collect::<Result<_, _>>()?;
            if nums.iter().all(|n| matches!(n, Num::I(_))) {
                let mut total: i64 = 0;
                for n in &nums {
                    if let Num::I(i) = n {
                        total = total.checked_add(*i).ok_or_else(|| {
                            Error::range("the sum is larger than 64 bits hold", span)
                        })?;
                    }
                }
                Ok(int(total))
            } else {
                Ok(ctx.float(
                    fsum(nums.iter().map(|n| n.f())),
                    "a result too large for a number",
                ))
            }
        }
        "any" | "all" => {
            a.arity(1, 1)?;
            let xs = match a.list(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(xs) => xs,
            };
            let mut unknown = false;
            for x in xs {
                match cond(x, span)? {
                    Some(true) if f == "any" => return Ok(Value::Bool(true)),
                    Some(false) if f == "all" => return Ok(Value::Bool(false)),
                    None => unknown = true,
                    _ => {}
                }
            }
            Ok(if unknown {
                Value::Null
            } else {
                Value::Bool(f == "all")
            })
        }
        "sorted" => {
            a.check_names(&["reverse"])?;
            a.arity(1, 1)?;
            let xs = match a.list(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(xs) => xs,
            };
            let reverse = match a.get(9, "reverse") {
                None | Some(Value::Bool(false)) => false,
                Some(Value::Bool(true)) => true,
                Some(_) => return Err(Error::type_("sorted()'s reverse is True or False", span)),
            };
            let mut out = xs.clone();
            let mut err = None;
            // A stable sort, as Python's is; reversing keeps equal
            // elements in their order, as `reverse=True` does.
            out.sort_by(|p, q| {
                let o = cmp(p, q).unwrap_or_else(|m| {
                    err.get_or_insert(m);
                    Some(Ordering::Equal)
                });
                let o = o.unwrap_or_else(|| {
                    err.get_or_insert("sorted() cannot order None".to_string());
                    Ordering::Equal
                });
                if reverse { o.reverse() } else { o }
            });
            match err {
                Some(m) => Err(Error::type_(m, span)),
                None => Ok(Value::Array(out)),
            }
        }
        "first" | "last" => {
            a.arity(1, 1)?;
            let xs = match a.list(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(xs) => xs,
            };
            Ok(if f == "first" { xs.first() } else { xs.last() }
                .cloned()
                .unwrap_or(Value::Null))
        }
        "count" => {
            a.arity(2, 2)?;
            let xs = match a.list(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(xs) => xs,
            };
            Ok(int(xs.iter().filter(|x| eq(x, &a.pos[1])).count() as i64))
        }
        "contains" => {
            a.arity(2, 2)?;
            match (a.string(&a.pos[0])?, a.string(&a.pos[1])?) {
                (Some(s), Some(p)) => Ok(Value::Bool(s.contains(p))),
                _ => Ok(Value::Null),
            }
        }
        "coalesce" => Ok(a
            .pos
            .iter()
            .find(|v| !v.is_null())
            .cloned()
            .unwrap_or(Value::Null)),
        "keys" | "values" => {
            a.arity(1, 1)?;
            match &a.pos[0] {
                Value::Null => Ok(Value::Null),
                Value::Object(m) => Ok(Value::Array(if f == "keys" {
                    m.keys().map(|k| Value::String(k.clone())).collect()
                } else {
                    m.values().cloned().collect()
                })),
                other => Err(Error::type_(format!("{f}() of {}", type_name(other)), span)),
            }
        }
        "has" => {
            a.arity(2, 2)?;
            match (&a.pos[0], &a.pos[1]) {
                (Value::Null, _) => Ok(Value::Bool(false)),
                (Value::Object(m), Value::String(k)) => Ok(Value::Bool(m.contains_key(k))),
                (other, _) => Err(Error::type_(
                    format!(
                        "has() looks in an object for a string key, not in {}",
                        type_name(other)
                    ),
                    span,
                )),
            }
        }
        "format" => {
            a.arity(2, 2)?;
            let spec = match &a.pos[1] {
                Value::String(s) => s,
                other => {
                    return Err(Error::type_(
                        format!("format()'s spec is a string, not {}", type_name(other)),
                        span,
                    ));
                }
            };
            match &a.pos[0] {
                Value::Null => Ok(Value::Null),
                v => match num(v) {
                    Some(n) => format_number(n.f(), matches!(n, Num::I(_)), spec)
                        .map(Value::String)
                        .map_err(|m| Error::type_(m, span)),
                    None => Err(Error::type_(
                        format!("format() takes a number, not {}", type_name(v)),
                        span,
                    )),
                },
            }
        }
        _ => {
            let near = FUNCTIONS
                .iter()
                .find(|n| n.starts_with(&f[..f.len().min(2)]));
            let hint = near.map_or(String::new(), |n| format!(" (did you mean {n}?)"));
            Err(Error::name(
                format!("there is no function {f}(){hint}"),
                span,
            ))
        }
    }
}

pub fn method(
    t: &Value,
    m: &str,
    pos: Vec<Value>,
    named: Vec<(String, Value)>,
    span: Span,
    _ctx: &mut Ctx,
) -> Result<Value, Error> {
    let a = Args {
        f: m,
        pos,
        named,
        span,
    };
    if t.is_null() {
        return Ok(Value::Null);
    }
    match (t, m) {
        (Value::String(s), "lower") => a.arity(0, 0).map(|_| Value::String(s.to_lowercase())),
        (Value::String(s), "upper") => a.arity(0, 0).map(|_| Value::String(s.to_uppercase())),
        (Value::String(s), "strip" | "lstrip" | "rstrip") => {
            a.arity(0, 1)?;
            let chars: Option<Vec<char>> = match a.pos.first() {
                None | Some(Value::Null) => None,
                Some(Value::String(c)) => Some(c.chars().collect()),
                Some(other) => {
                    return Err(Error::type_(
                        format!("{m}() takes a string, not {}", type_name(other)),
                        span,
                    ));
                }
            };
            let pred = |c: char| {
                chars
                    .as_ref()
                    .map_or(c.is_whitespace(), |cs| cs.contains(&c))
            };
            Ok(Value::String(match m {
                "strip" => s.trim_matches(pred).to_string(),
                "lstrip" => s.trim_start_matches(pred).to_string(),
                _ => s.trim_end_matches(pred).to_string(),
            }))
        }
        (Value::String(s), "startswith" | "endswith") => {
            a.arity(1, 1)?;
            match a.string(&a.pos[0])? {
                None => Ok(Value::Null),
                Some(p) => Ok(Value::Bool(if m == "startswith" {
                    s.starts_with(p)
                } else {
                    s.ends_with(p)
                })),
            }
        }
        (Value::String(s), "replace") => {
            a.arity(2, 2)?;
            match (a.string(&a.pos[0])?, a.string(&a.pos[1])?) {
                (Some(x), Some(y)) if !x.is_empty() => Ok(Value::String(s.replace(x, y))),
                (Some(_), Some(_)) => Err(Error::range("replace() of an empty string", span)),
                _ => Ok(Value::Null),
            }
        }
        (Value::String(s), "split") => {
            a.arity(0, 1)?;
            let parts: Vec<Value> = match a.pos.first() {
                None | Some(Value::Null) => s
                    .split_whitespace()
                    .map(|p| Value::String(p.to_string()))
                    .collect(),
                Some(Value::String(sep)) if !sep.is_empty() => s
                    .split(sep.as_str())
                    .map(|p| Value::String(p.to_string()))
                    .collect(),
                Some(Value::String(_)) => {
                    return Err(Error::range("split() by an empty separator", span));
                }
                Some(other) => {
                    return Err(Error::type_(
                        format!("split() takes a string, not {}", type_name(other)),
                        span,
                    ));
                }
            };
            Ok(Value::Array(parts))
        }
        (Value::String(sep), "join") => {
            a.arity(1, 1)?;
            let xs = match a.list(&a.pos[0])? {
                None => return Ok(Value::Null),
                Some(xs) => xs,
            };
            let mut out = Vec::with_capacity(xs.len());
            for x in xs {
                match x {
                    Value::String(p) => out.push(p.clone()),
                    Value::Null => return Ok(Value::Null),
                    other => {
                        return Err(Error::type_(
                            format!("join() takes strings, not {}", type_name(other)),
                            span,
                        ));
                    }
                }
            }
            Ok(Value::String(out.join(sep)))
        }
        (Value::Array(xs), "index") => {
            a.arity(1, 1)?;
            xs.iter()
                .position(|x| eq(x, &a.pos[0]))
                .map(|i| int(i as i64))
                .ok_or_else(|| Error::range("index(): the value is not in the list", span))
        }
        (Value::Array(xs), "count") => {
            a.arity(1, 1)?;
            Ok(int(xs.iter().filter(|x| eq(x, &a.pos[0])).count() as i64))
        }
        (Value::Object(o), "get") => {
            a.check_names(&["default"])?;
            a.arity(1, 2)?;
            let default = a.get(1, "default").cloned().unwrap_or(Value::Null);
            match &a.pos[0] {
                Value::String(k) => Ok(o.get(k).cloned().unwrap_or(default)),
                Value::Null => Ok(default),
                Value::Bool(b) => Ok(o
                    .get(if *b { "true" } else { "false" })
                    .cloned()
                    .unwrap_or(default)),
                other => Err(Error::type_(
                    format!("get() takes a string key, not {}", type_name(other)),
                    span,
                )),
            }
        }
        (Value::Object(o), "keys") => a
            .arity(0, 0)
            .map(|_| Value::Array(o.keys().map(|k| Value::String(k.clone())).collect())),
        (Value::Object(o), "values") => a
            .arity(0, 0)
            .map(|_| Value::Array(o.values().cloned().collect())),
        (other, _) => Err(Error::name(
            format!("{} has no method {m}()", type_name(other)),
            span,
        )),
    }
}
