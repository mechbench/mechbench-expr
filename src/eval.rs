//! Evaluation: an expression and a scope to a value.

use crate::ast::{Arg, BinOp, CmpOp, CompKind, Expr, ExprKind, UnOp};
use crate::error::{Error, Span};
use crate::library;
use crate::parser::float;
use crate::value::{Num, cmp, eq, int, num, type_name};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::BTreeMap;

/// Steps one evaluation may take before it stops with a limit: far more
/// than any real expression over one record needs, far fewer than a
/// runaway comprehension over a huge list would spend.
pub const DEFAULT_FUEL: u64 = 1_000_000;

/// What an expression can read: a record (in record scope), the
/// protocol's params, the input's header, and the names a comprehension
/// binds.
pub struct Scope<'a> {
    pub record: Option<&'a Value>,
    pub params: &'a Value,
    pub header: &'a Value,
    locals: Vec<(String, Value)>,
}

impl<'a> Scope<'a> {
    pub fn new(record: Option<&'a Value>, params: &'a Value, header: &'a Value) -> Self {
        Scope {
            record,
            params,
            header,
            locals: Vec::new(),
        }
    }
}

/// The running state of evaluations: fuel, and the undefined numbers met
/// (a division by zero, a log of zero), counted by reason, which became
/// null.
pub struct Ctx {
    pub fuel: u64,
    pub undefined: BTreeMap<String, u64>,
}

impl Ctx {
    pub fn new(fuel: u64) -> Self {
        Ctx {
            fuel,
            undefined: BTreeMap::new(),
        }
    }
    pub fn spend(&mut self, span: Span) -> Result<(), Error> {
        if self.fuel == 0 {
            return Err(Error::limit("the evaluation ran out of fuel", span));
        }
        self.fuel -= 1;
        Ok(())
    }
    /// An undefined number: null, counted under its reason.
    pub fn undefined(&mut self, reason: &str) -> Value {
        *self.undefined.entry(reason.to_string()).or_insert(0) += 1;
        Value::Null
    }
    /// A float result: null (counted) when it is not finite.
    pub fn float(&mut self, f: f64, reason: &str) -> Value {
        if f.is_finite() {
            float(f)
        } else {
            self.undefined(reason)
        }
    }
}

pub fn eval(e: &Expr, scope: &mut Scope, ctx: &mut Ctx) -> Result<Value, Error> {
    ctx.spend(e.span)?;
    match &e.kind {
        ExprKind::Lit(v) => Ok(v.clone()),
        ExprKind::Name(n) => Ok(name(n, scope)),
        ExprKind::Attr(target, field) => {
            let t = eval(target, scope, ctx)?;
            match t {
                Value::Null => Ok(Value::Null),
                Value::Object(m) => Ok(m.get(field).cloned().unwrap_or(Value::Null)),
                other => Err(Error::type_(
                    format!("{} has no field `{field}`", type_name(&other)),
                    e.span,
                )),
            }
        }
        ExprKind::Index(target, index) => {
            let t = eval(target, scope, ctx)?;
            let i = eval(index, scope, ctx)?;
            subscript(&t, &i, e.span)
        }
        ExprKind::Slice(target, a, b, c) => {
            let t = eval(target, scope, ctx)?;
            let mut bound = |x: &Option<Box<Expr>>| -> Result<Option<i64>, Error> {
                match x {
                    None => Ok(None),
                    Some(x) => match eval(x, scope, ctx)? {
                        Value::Null => Ok(None),
                        v => match num(&v) {
                            Some(Num::I(i)) => Ok(Some(i)),
                            _ => Err(Error::type_(
                                format!("a slice bound is an integer, not {}", type_name(&v)),
                                x.span,
                            )),
                        },
                    },
                }
            };
            let (a, b, c) = (bound(a)?, bound(b)?, bound(c)?);
            slice(&t, a, b, c, e.span)
        }
        ExprKind::Call(f, args) => {
            let (pos, named) = arguments(args, scope, ctx)?;
            library::call(f, pos, named, e.span, ctx)
        }
        ExprKind::Method(target, m, args) => {
            let t = eval(target, scope, ctx)?;
            let (pos, named) = arguments(args, scope, ctx)?;
            library::method(&t, m, pos, named, e.span, ctx)
        }
        ExprKind::Unary(op, inner) => {
            let v = eval(inner, scope, ctx)?;
            match op {
                UnOp::Not => Ok(match cond(&v, inner.span)? {
                    Some(b) => Value::Bool(!b),
                    None => Value::Null,
                }),
                UnOp::Pos | UnOp::Neg => match (&v, num(&v)) {
                    (Value::Null, _) => Ok(Value::Null),
                    (_, Some(Num::I(i))) => {
                        if *op == UnOp::Pos {
                            Ok(int(i))
                        } else {
                            i.checked_neg().map(int).ok_or_else(|| {
                                Error::range("the integer is too large to negate", e.span)
                            })
                        }
                    }
                    (_, Some(Num::F(f))) => Ok(float(if *op == UnOp::Pos { f } else { -f })),
                    _ => Err(Error::type_(
                        format!("cannot negate {}", type_name(&v)),
                        e.span,
                    )),
                },
            }
        }
        ExprKind::Binary(op, l, r) => {
            let a = eval(l, scope, ctx)?;
            let b = eval(r, scope, ctx)?;
            binary(*op, &a, &b, e.span, ctx)
        }
        ExprKind::Compare(first, rest) => {
            let mut left = eval(first, scope, ctx)?;
            let mut result = Some(true);
            for (op, right) in rest {
                let right_v = eval(right, scope, ctx)?;
                let r = compare(*op, &left, &right_v, right.span)?;
                // A chain is its links joined by Kleene's `and`.
                result = match (result, r) {
                    (Some(false), _) | (_, Some(false)) => Some(false),
                    (Some(true), Some(true)) => Some(true),
                    _ => None,
                };
                if result == Some(false) {
                    return Ok(Value::Bool(false));
                }
                left = right_v;
            }
            Ok(result.map_or(Value::Null, Value::Bool))
        }
        ExprKind::And(l, r) => {
            let a = cond(&eval(l, scope, ctx)?, l.span)?;
            if a == Some(false) {
                return Ok(Value::Bool(false));
            }
            let b = cond(&eval(r, scope, ctx)?, r.span)?;
            Ok(match (a, b) {
                (_, Some(false)) => Value::Bool(false),
                (Some(true), Some(true)) => Value::Bool(true),
                _ => Value::Null,
            })
        }
        ExprKind::Or(l, r) => {
            let a = cond(&eval(l, scope, ctx)?, l.span)?;
            if a == Some(true) {
                return Ok(Value::Bool(true));
            }
            let b = cond(&eval(r, scope, ctx)?, r.span)?;
            Ok(match (a, b) {
                (_, Some(true)) => Value::Bool(true),
                (Some(false), Some(false)) => Value::Bool(false),
                _ => Value::Null,
            })
        }
        ExprKind::IfElse {
            then,
            cond: c,
            otherwise,
        } => {
            // Only a true condition takes the first branch: false and
            // null both take the second, as a filter drops both.
            if cond(&eval(c, scope, ctx)?, c.span)? == Some(true) {
                eval(then, scope, ctx)
            } else {
                eval(otherwise, scope, ctx)
            }
        }
        ExprKind::List(items) => {
            let mut out = Vec::with_capacity(items.len());
            for x in items {
                out.push(eval(x, scope, ctx)?);
            }
            Ok(Value::Array(out))
        }
        ExprKind::Dict(pairs) => {
            let mut m = Map::new();
            for (k, v) in pairs {
                let key = match eval(k, scope, ctx)? {
                    Value::String(s) => s,
                    other => {
                        return Err(Error::type_(
                            format!("an object's key is a string, not {}", type_name(&other)),
                            k.span,
                        ));
                    }
                };
                let val = eval(v, scope, ctx)?;
                m.insert(key, val);
            }
            Ok(Value::Object(m))
        }
        ExprKind::Comp {
            kind,
            elt,
            var,
            iter,
            cond: c,
        } => {
            let items = match eval(iter, scope, ctx)? {
                Value::Array(xs) => xs,
                Value::Null => {
                    return Ok(if *kind == CompKind::List {
                        Value::Null
                    } else {
                        Value::Array(Vec::new())
                    });
                }
                other => {
                    return Err(Error::type_(
                        format!(
                            "a comprehension runs over a list, not {}",
                            type_name(&other)
                        ),
                        iter.span,
                    ));
                }
            };
            let mut out = Vec::new();
            for item in items {
                scope.locals.push((var.clone(), item));
                let keep = match c {
                    None => true,
                    Some(c) => {
                        let v = eval(c, scope, ctx);
                        match v {
                            Ok(v) => match cond(&v, c.span) {
                                Ok(b) => b == Some(true),
                                Err(err) => {
                                    scope.locals.pop();
                                    return Err(err);
                                }
                            },
                            Err(err) => {
                                scope.locals.pop();
                                return Err(err);
                            }
                        }
                    }
                };
                let v = if keep {
                    Some(eval(elt, scope, ctx))
                } else {
                    None
                };
                scope.locals.pop();
                if let Some(v) = v {
                    out.push(v?);
                }
            }
            Ok(Value::Array(out))
        }
    }
}

/// A bare name: a comprehension's variable, a root (`record`, `params`,
/// `header`), or the record's own top-level field; in protocol scope, a
/// param. Missing is null.
fn name(n: &str, scope: &Scope) -> Value {
    if let Some((_, v)) = scope.locals.iter().rev().find(|(k, _)| k == n) {
        return v.clone();
    }
    match n {
        "record" => return scope.record.cloned().unwrap_or(Value::Null),
        "params" => return scope.params.clone(),
        "header" => return scope.header.clone(),
        _ => {}
    }
    match scope.record {
        Some(r) => r.get(n).cloned().unwrap_or(Value::Null),
        None => scope.params.get(n).cloned().unwrap_or(Value::Null),
    }
}

/// A call's arguments, evaluated: the positional ones, then the named.
type Arguments = (Vec<Value>, Vec<(String, Value)>);

fn arguments(args: &[Arg], scope: &mut Scope, ctx: &mut Ctx) -> Result<Arguments, Error> {
    let mut pos = Vec::new();
    let mut named = Vec::new();
    for a in args {
        let v = eval(&a.value, scope, ctx)?;
        match &a.name {
            Some(n) => named.push((n.clone(), v)),
            None => pos.push(v),
        }
    }
    Ok((pos, named))
}

/// A condition's truth: `True`, `False`, or unknown (`None`). Anything
/// else is not a condition.
pub fn cond(v: &Value, span: Span) -> Result<Option<bool>, Error> {
    match v {
        Value::Bool(b) => Ok(Some(*b)),
        Value::Null => Ok(None),
        other => Err(Error::type_(
            format!(
                "a condition is True, False or None, not {} (compare it: `x == 1`, `x != \"\"`)",
                type_name(other)
            ),
            span,
        )),
    }
}

fn subscript(t: &Value, i: &Value, span: Span) -> Result<Value, Error> {
    match (t, i) {
        (Value::Null, _) | (_, Value::Null) => Ok(Value::Null),
        (Value::Object(m), Value::String(k)) => Ok(m.get(k).cloned().unwrap_or(Value::Null)),
        (Value::Array(xs), _) => {
            let idx = index_of(i, xs.len(), span)?;
            Ok(xs[idx].clone())
        }
        (Value::String(s), _) => {
            let chars: Vec<char> = s.chars().collect();
            let idx = index_of(i, chars.len(), span)?;
            Ok(Value::String(chars[idx].to_string()))
        }
        (Value::Object(_), other) => Err(Error::type_(
            format!("an object is indexed by a string, not {}", type_name(other)),
            span,
        )),
        (other, _) => Err(Error::type_(
            format!("{} cannot be indexed", type_name(other)),
            span,
        )),
    }
}

fn index_of(i: &Value, len: usize, span: Span) -> Result<usize, Error> {
    let k = match num(i) {
        Some(Num::I(k)) => k,
        _ => {
            return Err(Error::type_(
                format!("an index is an integer, not {}", type_name(i)),
                span,
            ));
        }
    };
    let real = if k < 0 { len as i64 + k } else { k };
    if real < 0 || real >= len as i64 {
        return Err(Error::range(
            format!("index {k} is out of range for a length of {len}"),
            span,
        ));
    }
    Ok(real as usize)
}

fn slice(
    t: &Value,
    a: Option<i64>,
    b: Option<i64>,
    c: Option<i64>,
    span: Span,
) -> Result<Value, Error> {
    let step = c.unwrap_or(1);
    if step == 0 {
        return Err(Error::range("a slice's step cannot be zero", span));
    }
    let pick = |len: usize| -> Vec<usize> {
        let len = len as i64;
        let clamp = |x: i64, lo: i64, hi: i64| x.max(lo).min(hi);
        let norm = |x: i64| if x < 0 { x + len } else { x };
        let mut out = Vec::new();
        if step > 0 {
            let start = clamp(a.map_or(0, norm), 0, len);
            let stop = clamp(b.map_or(len, norm), 0, len);
            let mut i = start;
            while i < stop {
                out.push(i as usize);
                i += step;
            }
        } else {
            let start = clamp(a.map_or(len - 1, norm), -1, len - 1);
            let stop = clamp(
                b.map_or(-1, |x| if x < 0 { x + len } else { x }),
                -1,
                len - 1,
            );
            let mut i = start;
            while i > stop {
                out.push(i as usize);
                i += step;
            }
        }
        out
    };
    match t {
        Value::Null => Ok(Value::Null),
        Value::Array(xs) => Ok(Value::Array(
            pick(xs.len()).into_iter().map(|i| xs[i].clone()).collect(),
        )),
        Value::String(s) => {
            let chars: Vec<char> = s.chars().collect();
            Ok(Value::String(
                pick(chars.len()).into_iter().map(|i| chars[i]).collect(),
            ))
        }
        other => Err(Error::type_(
            format!("{} cannot be sliced", type_name(other)),
            span,
        )),
    }
}

fn compare(op: CmpOp, a: &Value, b: &Value, span: Span) -> Result<Option<bool>, Error> {
    Ok(match op {
        CmpOp::Eq => Some(eq(a, b)),
        CmpOp::Ne => Some(!eq(a, b)),
        CmpOp::Is => Some(a.is_null()),
        CmpOp::IsNot => Some(!a.is_null()),
        CmpOp::In | CmpOp::NotIn => {
            let r = match (a, b) {
                (_, Value::Null) => None,
                (x, Value::Array(xs)) => Some(xs.iter().any(|y| eq(x, y))),
                (Value::String(x), Value::String(s)) => Some(s.contains(x.as_str())),
                (Value::Null, Value::String(_)) => None,
                (Value::String(k), Value::Object(m)) => Some(m.contains_key(k)),
                (x, other) => {
                    return Err(Error::type_(
                        format!(
                            "`in` looks in a list, a string or an object, not {} (for {})",
                            type_name(other),
                            type_name(x)
                        ),
                        span,
                    ));
                }
            };
            if op == CmpOp::NotIn { r.map(|v| !v) } else { r }
        }
        _ => {
            let o = cmp(a, b).map_err(|m| Error::type_(m, span))?;
            o.map(|o| match op {
                CmpOp::Lt => o == Ordering::Less,
                CmpOp::Le => o != Ordering::Greater,
                CmpOp::Gt => o == Ordering::Greater,
                _ => o != Ordering::Less,
            })
        }
    })
}

pub fn binary(op: BinOp, a: &Value, b: &Value, span: Span, ctx: &mut Ctx) -> Result<Value, Error> {
    if a.is_null() || b.is_null() {
        return Ok(Value::Null);
    }
    if op == BinOp::Add {
        match (a, b) {
            (Value::String(x), Value::String(y)) => return Ok(Value::String(format!("{x}{y}"))),
            (Value::Array(x), Value::Array(y)) => {
                return Ok(Value::Array(x.iter().chain(y).cloned().collect()));
            }
            _ => {}
        }
    }
    let (x, y) = match (num(a), num(b)) {
        (Some(x), Some(y)) => (x, y),
        _ => {
            let sym = match op {
                BinOp::Add => "+",
                BinOp::Sub => "-",
                BinOp::Mul => "*",
                BinOp::Div => "/",
                BinOp::FloorDiv => "//",
                BinOp::Mod => "%",
                BinOp::Pow => "**",
            };
            return Err(Error::type_(
                format!(
                    "cannot {} {} {sym} {}",
                    "compute",
                    type_name(a),
                    type_name(b)
                ),
                span,
            ));
        }
    };
    let overflow = || Error::range("the integer result is larger than 64 bits hold", span);
    match op {
        BinOp::Add | BinOp::Sub | BinOp::Mul => match (x, y) {
            (Num::I(i), Num::I(j)) => {
                let r = match op {
                    BinOp::Add => i.checked_add(j),
                    BinOp::Sub => i.checked_sub(j),
                    _ => i.checked_mul(j),
                };
                r.map(int).ok_or_else(overflow)
            }
            _ => {
                let (p, q) = (x.f(), y.f());
                let r = match op {
                    BinOp::Add => p + q,
                    BinOp::Sub => p - q,
                    _ => p * q,
                };
                Ok(ctx.float(r, "a result too large for a number"))
            }
        },
        BinOp::Div => {
            if y.f() == 0.0 {
                return Ok(ctx.undefined("division by zero"));
            }
            Ok(ctx.float(x.f() / y.f(), "a result too large for a number"))
        }
        BinOp::FloorDiv | BinOp::Mod => {
            if y.f() == 0.0 {
                return Ok(ctx.undefined(if op == BinOp::Mod {
                    "modulo by zero"
                } else {
                    "division by zero"
                }));
            }
            match (x, y) {
                (Num::I(i), Num::I(j)) => {
                    if i == i64::MIN && j == -1 {
                        return if op == BinOp::Mod {
                            Ok(int(0))
                        } else {
                            Err(overflow())
                        };
                    }
                    let r = i % j;
                    let adjust = r != 0 && ((r < 0) != (j < 0));
                    if op == BinOp::Mod {
                        Ok(int(if adjust { r + j } else { r }))
                    } else {
                        let q = i / j;
                        Ok(int(if adjust { q - 1 } else { q }))
                    }
                }
                _ => {
                    let (div, m) = float_divmod(x.f(), y.f());
                    Ok(ctx.float(
                        if op == BinOp::Mod { m } else { div },
                        "a result too large for a number",
                    ))
                }
            }
        }
        BinOp::Pow => match (x, y) {
            (Num::I(i), Num::I(j)) if j >= 0 => {
                let e = u32::try_from(j).map_err(|_| overflow())?;
                i.checked_pow(e).map(int).ok_or_else(overflow)
            }
            _ => {
                let (p, q) = (x.f(), y.f());
                if p == 0.0 && q < 0.0 {
                    return Ok(ctx.undefined("zero to a negative power"));
                }
                if p < 0.0 && q.fract() != 0.0 {
                    return Ok(ctx.undefined("a negative number to a fractional power"));
                }
                Ok(ctx.float(libm::pow(p, q), "a result too large for a number"))
            }
        },
    }
}

/// Python's float divmod: the floored quotient and the remainder with
/// the divisor's sign.
fn float_divmod(vx: f64, wx: f64) -> (f64, f64) {
    let mut m = libm::fmod(vx, wx);
    let mut div = (vx - m) / wx;
    if m != 0.0 {
        if (wx < 0.0) != (m < 0.0) {
            m += wx;
            div -= 1.0;
        }
    } else {
        m = libm::copysign(0.0, wx);
    }
    let floordiv = if div != 0.0 {
        let f = libm::floor(div);
        if div - f > 0.5 { f + 1.0 } else { f }
    } else {
        libm::copysign(0.0, vx / wx)
    };
    (floordiv, m)
}
