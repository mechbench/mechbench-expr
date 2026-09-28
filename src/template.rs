//! Templates: a string with expressions in braces, `"{coords.fact}:
//! {round(p, 3)}"`, and an optional format after a colon, `{p:.3f}`.
//! `{{` and `}}` are literal braces.

use crate::ast::Expr;
use crate::error::{Error, Span};
use crate::eval::{Ctx, Scope, eval};
use crate::parser::parse;
use crate::pyfmt::format_number;
use crate::value::{Num, num, py_str};
use serde_json::Value;

pub enum Part {
    Text(String),
    Field {
        expr: Expr,
        spec: Option<String>,
        offset: usize,
    },
}

pub fn parse_template(src: &str) -> Result<Vec<Part>, Error> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut parts = Vec::new();
    let mut text = String::new();
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        match c {
            '{' if chars.get(i + 1).map(|c| c.1) == Some('{') => {
                text.push('{');
                i += 2;
            }
            '}' if chars.get(i + 1).map(|c| c.1) == Some('}') => {
                text.push('}');
                i += 2;
            }
            '}' => {
                return Err(Error::syntax(
                    "a `}` with no `{`; write `}}` for a brace",
                    Span::new(at, at + 1),
                ));
            }
            '{' => {
                // Find the closing brace at depth zero, outside strings, and
                // the format's colon, the first at depth zero.
                let mut depth = 0i32;
                let mut quote: Option<char> = None;
                let mut colon: Option<usize> = None;
                let mut j = i + 1;
                let mut end = None;
                while j < chars.len() {
                    let (_, d) = chars[j];
                    match quote {
                        Some(q) => {
                            if d == '\\' {
                                j += 1;
                            } else if d == q {
                                quote = None;
                            }
                        }
                        None => match d {
                            '"' | '\'' => quote = Some(d),
                            '(' | '[' | '{' => depth += 1,
                            ')' | ']' => depth -= 1,
                            '}' if depth == 0 => {
                                end = Some(j);
                                break;
                            }
                            '}' => depth -= 1,
                            ':' if depth == 0 && colon.is_none() => colon = Some(j),
                            _ => {}
                        },
                    }
                    j += 1;
                }
                let end = end.ok_or_else(|| {
                    Error::syntax(
                        "a `{` is not closed; write `{{` for a brace",
                        Span::new(at, src.len()),
                    )
                })?;
                if !text.is_empty() {
                    parts.push(Part::Text(std::mem::take(&mut text)));
                }
                let byte = |k: usize| chars.get(k).map_or(src.len(), |c| c.0);
                let body_end = colon.unwrap_or(end);
                let body = &src[byte(i + 1)..byte(body_end)];
                let expr = parse(body).map_err(|e| Error {
                    span: Span::new(e.span.start + byte(i + 1), e.span.end + byte(i + 1)),
                    ..e
                })?;
                let spec = colon.map(|c| src[byte(c + 1)..byte(end)].to_string());
                parts.push(Part::Field {
                    expr,
                    spec,
                    offset: byte(i + 1),
                });
                i = end + 1;
            }
            _ => {
                text.push(c);
                i += 1;
            }
        }
    }
    if !text.is_empty() {
        parts.push(Part::Text(text));
    }
    Ok(parts)
}

pub fn render(parts: &[Part], scope: &mut Scope, ctx: &mut Ctx) -> Result<String, Error> {
    let mut out = String::new();
    for p in parts {
        match p {
            Part::Text(t) => out.push_str(t),
            Part::Field { expr, spec, offset } => {
                let v = eval(expr, scope, ctx)?;
                match spec {
                    None => out.push_str(&py_str(&v)),
                    Some(spec) => {
                        let n = num(&v);
                        match (&v, n) {
                            (_, Some(n)) => {
                                let s = format_number(n.f(), matches!(n, Num::I(_)), spec)
                                    .map_err(|m| Error::type_(m, Span::new(*offset, *offset)))?;
                                out.push_str(&s);
                            }
                            (Value::Null, _) => out.push_str("None"),
                            _ => out.push_str(&py_str(&v)),
                        }
                    }
                }
            }
        }
    }
    Ok(out)
}
