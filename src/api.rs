//! The engine's one entry point, JSON in and JSON out, so every host
//! (wasmtime, Node, a browser) drives it the same way and a whole
//! collection crosses the boundary once.
//!
//! Requests:
//!
//! - `{"op": "check", "expr": "..."}` → the canonical form and the field
//!   paths it reads, or the syntax error;
//! - `{"op": "eval", "expr": "..." | "exprs": {"name": "..."}, "records":
//!   [...] | "record": {...}, "params": {...}, "header": {...}, "fuel": n}`
//!   → one value per record (an object per record with `exprs`), and the
//!   undefined numbers met, by reason;
//! - `{"op": "filter", "expr": "...", "records": [...]}` → the indexes of
//!   the records whose condition is `True`, and how many were null;
//! - `{"op": "template", "template": "...", "records": [...]}` → one string
//!   per record;
//! - `{"op": "split", "expr": "wilson(correct, level=0.9)"}` → an
//!   aggregate call taken apart: its function, its positional arguments
//!   as canonical expressions (read per record), and its named arguments
//!   evaluated once in protocol scope.
//!
//! An error answers `{"ok": false, "error": {"kind", "message", "start",
//! "end", "record"}}`, `record` the index of the record it met.

use crate::ast::ExprKind;
use crate::canon::{canonical, reads};
use crate::error::Error;
use crate::eval::{Ctx, DEFAULT_FUEL, Scope, cond, eval};
use crate::parser::parse;
use crate::template::{parse_template, render};
use serde_json::{Map, Value, json};

pub fn handle(request: &str) -> String {
    let answer = match serde_json::from_str::<Value>(request) {
        Ok(req) => dispatch(&req),
        Err(e) => {
            json!({"ok": false, "error": {"kind": "request", "message": format!("the request is not JSON: {e}")}})
        }
    };
    answer.to_string()
}

fn fail(e: &Error, record: Option<usize>) -> Value {
    json!({
        "ok": false,
        "error": {"kind": e.kind_name(), "message": e.message, "start": e.span.start, "end": e.span.end, "record": record},
    })
}

fn bad(message: &str) -> Value {
    json!({"ok": false, "error": {"kind": "request", "message": message}})
}

fn dispatch(req: &Value) -> Value {
    let op = req.get("op").and_then(Value::as_str).unwrap_or("");
    let empty = Value::Object(Map::new());
    let params = req.get("params").unwrap_or(&empty);
    let header = req.get("header").unwrap_or(&empty);
    let fuel = req
        .get("fuel")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_FUEL);
    let records: Option<Vec<&Value>> = match (req.get("records"), req.get("record")) {
        (Some(Value::Array(rs)), _) => Some(rs.iter().collect()),
        (_, Some(r)) => Some(vec![r]),
        _ => None,
    };
    match op {
        "check" => {
            let src = match req.get("expr").and_then(Value::as_str) {
                Some(s) => s,
                None => return bad("check takes `expr`, a string"),
            };
            match parse(src) {
                Ok(e) => {
                    json!({"ok": true, "canonical": canonical(&e), "reads": reads(&e).into_iter().collect::<Vec<_>>()})
                }
                Err(e) => fail(&e, None),
            }
        }
        "eval" | "filter" => {
            let named: Vec<(Option<String>, String)> = match (req.get("expr"), req.get("exprs")) {
                (Some(Value::String(s)), _) => vec![(None, s.clone())],
                (_, Some(Value::Object(m))) if op == "eval" => {
                    let mut out = Vec::new();
                    for (k, v) in m {
                        match v.as_str() {
                            Some(s) => out.push((Some(k.clone()), s.to_string())),
                            None => return bad("each of `exprs` is a string"),
                        }
                    }
                    out
                }
                _ => {
                    return bad(
                        "eval takes `expr` (a string) or `exprs` (an object of strings); filter takes `expr`",
                    );
                }
            };
            let mut parsed = Vec::with_capacity(named.len());
            for (name, src) in &named {
                match parse(src) {
                    Ok(e) => parsed.push((name.clone(), e)),
                    Err(e) => return fail(&e, None),
                }
            }
            let mut ctx = Ctx::new(fuel);
            let rows: Vec<Option<&Value>> = match &records {
                Some(rs) => rs.iter().map(|r| Some(*r)).collect(),
                None => vec![None],
            };
            let mut values = Vec::with_capacity(rows.len());
            let mut kept = Vec::new();
            let mut unknown = 0u64;
            for (i, rec) in rows.iter().enumerate() {
                let mut scope = Scope::new(*rec, params, header);
                if op == "filter" {
                    let (_, e) = &parsed[0];
                    let v = match eval(e, &mut scope, &mut ctx) {
                        Ok(v) => v,
                        Err(err) => return fail(&err, records.as_ref().map(|_| i)),
                    };
                    match cond(&v, e.span) {
                        Ok(Some(true)) => kept.push(i),
                        Ok(Some(false)) => {}
                        Ok(None) => unknown += 1,
                        Err(err) => return fail(&err, records.as_ref().map(|_| i)),
                    }
                    continue;
                }
                if parsed.len() == 1 && parsed[0].0.is_none() {
                    match eval(&parsed[0].1, &mut scope, &mut ctx) {
                        Ok(v) => values.push(v),
                        Err(err) => return fail(&err, records.as_ref().map(|_| i)),
                    }
                } else {
                    let mut row = Map::new();
                    for (name, e) in &parsed {
                        match eval(e, &mut scope, &mut ctx) {
                            Ok(v) => {
                                row.insert(name.clone().unwrap_or_default(), v);
                            }
                            Err(err) => return fail(&err, records.as_ref().map(|_| i)),
                        }
                    }
                    values.push(Value::Object(row));
                }
            }
            let undefined: Map<String, Value> = ctx
                .undefined
                .into_iter()
                .map(|(k, v)| (k, Value::from(v)))
                .collect();
            if op == "filter" {
                json!({"ok": true, "kept": kept, "unknown": unknown, "undefined": undefined})
            } else {
                json!({"ok": true, "values": values, "undefined": undefined})
            }
        }
        "template" => {
            let src = match req.get("template").and_then(Value::as_str) {
                Some(s) => s,
                None => return bad("template takes `template`, a string"),
            };
            let parts = match parse_template(src) {
                Ok(p) => p,
                Err(e) => return fail(&e, None),
            };
            let mut ctx = Ctx::new(fuel);
            let rows: Vec<Option<&Value>> = match &records {
                Some(rs) => rs.iter().map(|r| Some(*r)).collect(),
                None => vec![None],
            };
            let mut out = Vec::with_capacity(rows.len());
            for (i, rec) in rows.iter().enumerate() {
                let mut scope = Scope::new(*rec, params, header);
                match render(&parts, &mut scope, &mut ctx) {
                    Ok(s) => out.push(Value::String(s)),
                    Err(err) => return fail(&err, records.as_ref().map(|_| i)),
                }
            }
            let undefined: Map<String, Value> = ctx
                .undefined
                .into_iter()
                .map(|(k, v)| (k, Value::from(v)))
                .collect();
            json!({"ok": true, "values": out, "undefined": undefined})
        }
        "split" => {
            let src = match req.get("expr").and_then(Value::as_str) {
                Some(s) => s,
                None => return bad("split takes `expr`, a string"),
            };
            let e = match parse(src) {
                Ok(e) => e,
                Err(e) => return fail(&e, None),
            };
            let ExprKind::Call(function, args) = &e.kind else {
                let err = Error::syntax(
                    "an aggregate is one call, such as `mean(x)` or `wilson(correct, level=0.9)`",
                    e.span,
                );
                return fail(&err, None);
            };
            // The positional arguments are read per record; the named ones
            // are settings, read once in protocol scope.
            let mut positional = Vec::new();
            let mut named = Map::new();
            let mut ctx = Ctx::new(fuel);
            for a in args {
                match &a.name {
                    None => positional.push(Value::String(canonical(&a.value))),
                    Some(n) => {
                        let mut scope = Scope::new(None, params, header);
                        match eval(&a.value, &mut scope, &mut ctx) {
                            Ok(v) => {
                                named.insert(n.clone(), v);
                            }
                            Err(err) => return fail(&err, None),
                        }
                    }
                }
            }
            json!({"ok": true, "function": function, "args": positional, "named": named, "canonical": canonical(&e)})
        }
        _ => bad("op is check, eval, filter, template or split"),
    }
}

// The WebAssembly surface: the host allocates a buffer in the module's
// memory, writes the request's UTF-8 there, calls `mbexpr_call`, reads
// the answer from the pointer and length packed in the result, and frees
// both buffers.

fn layout(len: usize) -> std::alloc::Layout {
    std::alloc::Layout::array::<u8>(len.max(1)).unwrap_or(std::alloc::Layout::new::<u8>())
}

#[unsafe(no_mangle)]
pub extern "C" fn mbexpr_alloc(len: usize) -> *mut u8 {
    unsafe { std::alloc::alloc(layout(len)) }
}

/// # Safety
/// `ptr` and `len` are a buffer `mbexpr_alloc` returned, or an answer
/// `mbexpr_call` returned, freed once, with the length it was made with.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbexpr_free(ptr: *mut u8, len: usize) {
    if !ptr.is_null() {
        unsafe { std::alloc::dealloc(ptr, layout(len)) };
    }
}

/// # Safety
/// `ptr` and `len` are a buffer `mbexpr_alloc` returned, holding `len`
/// bytes of UTF-8 written by the host.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn mbexpr_call(ptr: *const u8, len: usize) -> u64 {
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) };
    let answer = match std::str::from_utf8(bytes) {
        Ok(s) => handle(s),
        Err(_) => bad("the request is not UTF-8").to_string(),
    };
    let n = answer.len();
    let out = mbexpr_alloc(n);
    unsafe { std::ptr::copy_nonoverlapping(answer.as_ptr(), out, n) };
    ((out as u64) << 32) | n as u64
}
