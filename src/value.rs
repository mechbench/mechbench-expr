//! Values are JSON's. These are the rules for reading them as numbers,
//! comparing them and printing them.

use crate::parser::float;
use crate::pyfmt::float_repr;
use serde_json::{Number, Value};
use std::cmp::Ordering;

#[derive(Debug, Clone, Copy)]
pub enum Num {
    I(i64),
    F(f64),
}

impl Num {
    pub fn f(self) -> f64 {
        match self {
            Num::I(i) => i as f64,
            Num::F(f) => f,
        }
    }
}

/// A value as a number, when it is one. A boolean is not a number here
/// (arithmetic on `True` is a type error), except where `eq` and `cmp`
/// say otherwise.
pub fn num(v: &Value) -> Option<Num> {
    match v {
        Value::Number(n) => Some(if let Some(i) = n.as_i64() {
            Num::I(i)
        } else {
            Num::F(n.as_f64().unwrap_or(f64::NAN))
        }),
        _ => None,
    }
}

pub fn int(i: i64) -> Value {
    Value::Number(Number::from(i))
}

pub fn of_num(n: Num) -> Value {
    match n {
        Num::I(i) => int(i),
        Num::F(f) => float(f),
    }
}

/// The name of a value's type, for messages.
pub fn type_name(v: &Value) -> &'static str {
    match v {
        Value::Null => "None",
        Value::Bool(_) => "a boolean",
        Value::Number(n) if n.is_i64() => "an integer",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "a list",
        Value::Object(_) => "an object",
    }
}

/// A number, or a boolean read as 1 or 0, for equality and ordering.
fn numeric(v: &Value) -> Option<f64> {
    match v {
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        Value::Number(_) => num(v).map(Num::f),
        _ => None,
    }
}

/// Structural equality: numbers by value (`1 == 1.0`, `1 == True`),
/// strings, lists and objects element by element.
pub fn eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Null, Value::Null) => true,
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| eq(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, v)| y.get(k).is_some_and(|w| eq(v, w)))
        }
        _ => match (numeric(a), numeric(b)) {
            (Some(x), Some(y)) => match (num(a), num(b)) {
                (Some(Num::I(i)), Some(Num::I(j))) => i == j,
                _ => x == y,
            },
            _ => false,
        },
    }
}

/// Ordering of two numbers, two strings (by code point) or two lists;
/// `Ok(None)` when either is null; an error for anything else.
pub fn cmp(a: &Value, b: &Value) -> Result<Option<Ordering>, String> {
    match (a, b) {
        (Value::Null, _) | (_, Value::Null) => Ok(None),
        (Value::String(x), Value::String(y)) => Ok(Some(x.as_str().cmp(y.as_str()))),
        (Value::Array(x), Value::Array(y)) => {
            for (p, q) in x.iter().zip(y) {
                match cmp(p, q)? {
                    Some(Ordering::Equal) => continue,
                    other => return Ok(other),
                }
            }
            Ok(Some(x.len().cmp(&y.len())))
        }
        _ => match (num(a), num(b)) {
            (Some(Num::I(i)), Some(Num::I(j))) => Ok(Some(i.cmp(&j))),
            _ => match (numeric(a), numeric(b)) {
                (Some(x), Some(y)) => Ok(x.partial_cmp(&y)),
                _ => Err(format!(
                    "cannot order {} and {}",
                    type_name(a),
                    type_name(b)
                )),
            },
        },
    }
}

/// `str(x)` as Python prints it.
pub fn py_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => py_repr(other),
    }
}

/// `repr(x)` as Python prints it: strings quoted, lists and objects in
/// Python's own brackets.
pub fn py_repr(v: &Value) -> String {
    match v {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::Number(n) => match n.as_i64() {
            Some(i) => i.to_string(),
            None => float_repr(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(s) => quote(s),
        Value::Array(xs) => format!(
            "[{}]",
            xs.iter().map(py_repr).collect::<Vec<_>>().join(", ")
        ),
        Value::Object(m) => format!(
            "{{{}}}",
            m.iter()
                .map(|(k, v)| format!("{}: {}", quote(k), py_repr(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

fn quote(s: &str) -> String {
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len() + 2);
    out.push(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}
