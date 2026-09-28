//! Python's expression grammar, for the productions the language keeps.
//! Lowest to highest: conditional, `or`, `and`, `not`, comparison
//! (chained), `+ -`, `* / // %`, unary `- +`, `**`, then calls,
//! attributes and subscripts on an atom.

use crate::ast::{Arg, BinOp, CmpOp, CompKind, Expr, ExprKind, UnOp};
use crate::error::{Error, Span};
use crate::lexer::{Tok, Token, lex};
use serde_json::{Number, Value};

/// How deeply an expression may nest, in the parser and in the finished
/// tree (evaluation recurses on it): far beyond any real expression,
/// well inside a WebAssembly stack.
pub const MAX_DEPTH: usize = 64;

pub fn parse(src: &str) -> Result<Expr, Error> {
    let tokens = lex(src)?;
    let mut p = Parser {
        tokens,
        i: 0,
        depth: 0,
    };
    let e = p.expr()?;
    let t = p.peek();
    if t.tok != Tok::End {
        return Err(Error::syntax(
            "unexpected text after the expression",
            t.span,
        ));
    }
    if depth(&e) > MAX_DEPTH {
        return Err(Error::limit(
            format!("the expression nests deeper than {MAX_DEPTH} levels"),
            e.span,
        ));
    }
    Ok(e)
}

/// The tree's depth, found without recursing: a long chain such as
/// `1 + 1 + ... + 1` parses in a loop but is deep.
fn depth(root: &Expr) -> usize {
    let mut max = 0;
    let mut stack = vec![(root, 1usize)];
    while let Some((e, d)) = stack.pop() {
        max = max.max(d);
        if max > MAX_DEPTH {
            return max;
        }
        for c in children(e) {
            stack.push((c, d + 1));
        }
    }
    max
}

fn children(e: &Expr) -> Vec<&Expr> {
    match &e.kind {
        ExprKind::Lit(_) | ExprKind::Name(_) => vec![],
        ExprKind::Attr(t, _) | ExprKind::Unary(_, t) => vec![t],
        ExprKind::Index(t, i) | ExprKind::Binary(_, t, i) => vec![t, i],
        ExprKind::Chain(f, rest) => std::iter::once(&**f)
            .chain(rest.iter().map(|(_, x)| x))
            .collect(),
        ExprKind::And(xs) | ExprKind::Or(xs) => xs.iter().collect(),
        ExprKind::Slice(t, a, b, c) => {
            let mut out: Vec<&Expr> = vec![t];
            out.extend([a, b, c].into_iter().flatten().map(|x| &**x));
            out
        }
        ExprKind::Call(_, args) => args.iter().map(|a| &a.value).collect(),
        ExprKind::Method(t, _, args) => std::iter::once(&**t)
            .chain(args.iter().map(|a| &a.value))
            .collect(),
        ExprKind::Compare(f, rest) => std::iter::once(&**f)
            .chain(rest.iter().map(|(_, x)| x))
            .collect(),
        ExprKind::IfElse {
            then,
            cond,
            otherwise,
        } => vec![then, cond, otherwise],
        ExprKind::List(xs) => xs.iter().collect(),
        ExprKind::Dict(ps) => ps.iter().flat_map(|(k, v)| [k, v]).collect(),
        ExprKind::Comp {
            elt, iter, cond, ..
        } => {
            let mut out: Vec<&Expr> = vec![elt, iter];
            if let Some(c) = cond {
                out.push(c);
            }
            out
        }
    }
}

struct Parser {
    tokens: Vec<Token>,
    i: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.i]
    }
    fn peek_at(&self, k: usize) -> &Tok {
        &self.tokens[(self.i + k).min(self.tokens.len() - 1)].tok
    }
    fn next(&mut self) -> Token {
        let t = self.tokens[self.i].clone();
        if self.i < self.tokens.len() - 1 {
            self.i += 1;
        }
        t
    }
    fn eat(&mut self, tok: &Tok) -> bool {
        if &self.peek().tok == tok {
            self.next();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, tok: Tok, what: &str) -> Result<Token, Error> {
        if self.peek().tok == tok {
            Ok(self.next())
        } else {
            Err(Error::syntax(format!("expected {what}"), self.peek().span))
        }
    }
    fn enter(&mut self) -> Result<(), Error> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Error::limit(
                "the expression nests too deeply",
                self.peek().span,
            ));
        }
        Ok(())
    }

    fn expr(&mut self) -> Result<Expr, Error> {
        self.enter()?;
        let then = self.or()?;
        let out = if self.peek().tok == Tok::If {
            self.next();
            let cond = self.or()?;
            self.expect(Tok::Else, "`else` after the condition")?;
            let otherwise = self.expr()?;
            let span = then.span.to(otherwise.span);
            Expr::new(
                ExprKind::IfElse {
                    then: Box::new(then),
                    cond: Box::new(cond),
                    otherwise: Box::new(otherwise),
                },
                span,
            )
        } else {
            then
        };
        self.depth -= 1;
        Ok(out)
    }

    fn or(&mut self) -> Result<Expr, Error> {
        let first = self.and()?;
        let mut items = vec![first];
        while self.eat(&Tok::Or) {
            items.push(self.and()?);
        }
        Ok(join(items, ExprKind::Or))
    }

    fn and(&mut self) -> Result<Expr, Error> {
        let first = self.not()?;
        let mut items = vec![first];
        while self.eat(&Tok::And) {
            items.push(self.not()?);
        }
        Ok(join(items, ExprKind::And))
    }

    fn not(&mut self) -> Result<Expr, Error> {
        if self.peek().tok == Tok::Not {
            let start = self.next().span;
            self.enter()?;
            let inner = self.not()?;
            self.depth -= 1;
            let span = start.to(inner.span);
            return Ok(Expr::new(ExprKind::Unary(UnOp::Not, Box::new(inner)), span));
        }
        self.comparison()
    }

    fn comparison(&mut self) -> Result<Expr, Error> {
        let first = self.sum()?;
        let mut rest = Vec::new();
        loop {
            let op = match (&self.peek().tok, self.peek_at(1)) {
                (Tok::Eq, _) => CmpOp::Eq,
                (Tok::Ne, _) => CmpOp::Ne,
                (Tok::Lt, _) => CmpOp::Lt,
                (Tok::Le, _) => CmpOp::Le,
                (Tok::Gt, _) => CmpOp::Gt,
                (Tok::Ge, _) => CmpOp::Ge,
                (Tok::In, _) => CmpOp::In,
                (Tok::Not, Tok::In) => CmpOp::NotIn,
                (Tok::Is, Tok::Not) => CmpOp::IsNot,
                (Tok::Is, _) => CmpOp::Is,
                (Tok::Assign, _) => {
                    return Err(Error::syntax(
                        "`=` is assignment; compare with `==`",
                        self.peek().span,
                    ));
                }
                _ => break,
            };
            let two = matches!(op, CmpOp::NotIn | CmpOp::IsNot);
            self.next();
            if two {
                self.next();
            }
            let right = self.sum()?;
            if matches!(op, CmpOp::Is | CmpOp::IsNot)
                && !matches!(right.kind, ExprKind::Lit(Value::Null))
            {
                return Err(Error::syntax(
                    "`is` compares with None only; use `==` for values",
                    right.span,
                ));
            }
            rest.push((op, right));
        }
        if rest.is_empty() {
            return Ok(first);
        }
        let span = first.span.to(rest.last().map_or(first.span, |r| r.1.span));
        Ok(Expr::new(ExprKind::Compare(Box::new(first), rest), span))
    }

    fn sum(&mut self) -> Result<Expr, Error> {
        let first = self.term()?;
        let mut rest = Vec::new();
        loop {
            let op = match self.peek().tok {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => break,
            };
            self.next();
            rest.push((op, self.term()?));
        }
        Ok(chain(first, rest))
    }

    fn term(&mut self) -> Result<Expr, Error> {
        let first = self.unary()?;
        let mut rest = Vec::new();
        loop {
            let op = match self.peek().tok {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::SlashSlash => BinOp::FloorDiv,
                Tok::Percent => BinOp::Mod,
                _ => break,
            };
            self.next();
            rest.push((op, self.unary()?));
        }
        Ok(chain(first, rest))
    }

    fn unary(&mut self) -> Result<Expr, Error> {
        let op = match self.peek().tok {
            Tok::Minus => UnOp::Neg,
            Tok::Plus => UnOp::Pos,
            _ => return self.power(),
        };
        let start = self.next().span;
        self.enter()?;
        let inner = self.unary()?;
        self.depth -= 1;
        let span = start.to(inner.span);
        Ok(Expr::new(ExprKind::Unary(op, Box::new(inner)), span))
    }

    fn power(&mut self) -> Result<Expr, Error> {
        let base = self.postfix()?;
        if self.eat(&Tok::StarStar) {
            self.enter()?;
            // Right-associative, and binds a unary minus on its right:
            // `2 ** -1`.
            let exp = self.unary()?;
            self.depth -= 1;
            let span = base.span.to(exp.span);
            return Ok(Expr::new(
                ExprKind::Binary(BinOp::Pow, Box::new(base), Box::new(exp)),
                span,
            ));
        }
        Ok(base)
    }

    fn postfix(&mut self) -> Result<Expr, Error> {
        let mut e = self.atom()?;
        loop {
            match self.peek().tok {
                Tok::Dot => {
                    self.next();
                    let t = self.next();
                    let name = match t.tok {
                        Tok::Name(n) => n,
                        _ => return Err(Error::syntax("expected a field name after `.`", t.span)),
                    };
                    if self.peek().tok == Tok::LParen {
                        let (args, end) = self.args()?;
                        let span = e.span.to(end);
                        e = Expr::new(ExprKind::Method(Box::new(e), name, args), span);
                    } else {
                        let span = e.span.to(t.span);
                        e = Expr::new(ExprKind::Attr(Box::new(e), name), span);
                    }
                }
                Tok::LBracket => {
                    self.next();
                    e = self.subscript(e)?;
                }
                Tok::LParen => {
                    return Err(Error::syntax(
                        "only the library's functions can be called",
                        self.peek().span,
                    ));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn subscript(&mut self, target: Expr) -> Result<Expr, Error> {
        let mut parts: [Option<Box<Expr>>; 3] = [None, None, None];
        let mut colons = 0;
        loop {
            match self.peek().tok {
                Tok::Colon => {
                    self.next();
                    colons += 1;
                    if colons > 2 {
                        return Err(Error::syntax(
                            "a slice has at most two `:`",
                            self.peek().span,
                        ));
                    }
                }
                Tok::RBracket => break,
                _ => {
                    if parts[colons].is_some() {
                        return Err(Error::syntax("expected `:` or `]`", self.peek().span));
                    }
                    parts[colons] = Some(Box::new(self.expr()?));
                }
            }
        }
        let end = self.expect(Tok::RBracket, "`]`")?.span;
        let span = target.span.to(end);
        if colons == 0 {
            let index = parts[0]
                .take()
                .ok_or_else(|| Error::syntax("expected an index", end))?;
            return Ok(Expr::new(ExprKind::Index(Box::new(target), index), span));
        }
        let [a, b, c] = parts;
        Ok(Expr::new(ExprKind::Slice(Box::new(target), a, b, c), span))
    }

    /// A call's arguments, `(...)`, with a generator allowed as the sole
    /// argument: `sum(x.p for x in top)`.
    fn args(&mut self) -> Result<(Vec<Arg>, Span), Error> {
        self.expect(Tok::LParen, "`(`")?;
        let mut args = Vec::new();
        let mut named = false;
        while self.peek().tok != Tok::RParen {
            if let (Tok::Name(n), Tok::Assign) = (self.peek().tok.clone(), self.peek_at(1).clone())
            {
                self.next();
                self.next();
                let value = self.expr()?;
                args.push(Arg {
                    name: Some(n),
                    value,
                });
                named = true;
            } else {
                if named {
                    return Err(Error::syntax(
                        "a positional argument follows a named one",
                        self.peek().span,
                    ));
                }
                let value = self.expr()?;
                if self.peek().tok == Tok::For {
                    let value = self.comprehension(value, CompKind::Generator)?;
                    args.push(Arg { name: None, value });
                } else {
                    args.push(Arg { name: None, value });
                }
            }
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        let end = self.expect(Tok::RParen, "`)`")?.span;
        Ok((args, end))
    }

    fn comprehension(&mut self, elt: Expr, kind: CompKind) -> Result<Expr, Error> {
        self.expect(Tok::For, "`for`")?;
        let t = self.next();
        let var = match t.tok {
            Tok::Name(n) => n,
            _ => return Err(Error::syntax("expected one name after `for`", t.span)),
        };
        self.expect(Tok::In, "`in`")?;
        let iter = self.or()?;
        let cond = if self.eat(&Tok::If) {
            Some(Box::new(self.or()?))
        } else {
            None
        };
        if self.peek().tok == Tok::For {
            return Err(Error::syntax(
                "a comprehension has one `for`",
                self.peek().span,
            ));
        }
        let span = elt.span.to(cond.as_ref().map_or(iter.span, |c| c.span));
        Ok(Expr::new(
            ExprKind::Comp {
                kind,
                elt: Box::new(elt),
                var,
                iter: Box::new(iter),
                cond,
            },
            span,
        ))
    }

    fn atom(&mut self) -> Result<Expr, Error> {
        let t = self.next();
        let span = t.span;
        let kind = match t.tok {
            Tok::Int(v) => ExprKind::Lit(Value::Number(Number::from(v))),
            Tok::Float(v) => ExprKind::Lit(float(v)),
            Tok::Str(s) => {
                // Adjacent strings join, as in Python: "a" "b".
                let mut s = s;
                let mut end = span;
                while let Tok::Str(more) = &self.peek().tok {
                    s.push_str(more);
                    end = self.next().span;
                }
                return Ok(Expr::new(ExprKind::Lit(Value::String(s)), span.to(end)));
            }
            Tok::True => ExprKind::Lit(Value::Bool(true)),
            Tok::False => ExprKind::Lit(Value::Bool(false)),
            Tok::None => ExprKind::Lit(Value::Null),
            Tok::Name(n) => {
                if self.peek().tok == Tok::LParen {
                    let (args, end) = self.args()?;
                    return Ok(Expr::new(ExprKind::Call(n, args), span.to(end)));
                }
                ExprKind::Name(n)
            }
            Tok::LParen => {
                self.enter()?;
                let inner = self.expr()?;
                self.depth -= 1;
                if self.peek().tok == Tok::Comma {
                    return Err(Error::syntax(
                        "tuples are not part of the expression language; use a list",
                        self.peek().span,
                    ));
                }
                let end = self.expect(Tok::RParen, "`)`")?.span;
                return Ok(Expr {
                    span: span.to(end),
                    ..inner
                });
            }
            Tok::LBracket => {
                self.enter()?;
                let mut items = Vec::new();
                if self.peek().tok != Tok::RBracket {
                    let first = self.expr()?;
                    if self.peek().tok == Tok::For {
                        let comp = self.comprehension(first, CompKind::List)?;
                        let end = self.expect(Tok::RBracket, "`]`")?.span;
                        self.depth -= 1;
                        return Ok(Expr {
                            span: span.to(end),
                            ..comp
                        });
                    }
                    items.push(first);
                    while self.eat(&Tok::Comma) {
                        if self.peek().tok == Tok::RBracket {
                            break;
                        }
                        items.push(self.expr()?);
                    }
                }
                let end = self.expect(Tok::RBracket, "`]`")?.span;
                self.depth -= 1;
                return Ok(Expr::new(ExprKind::List(items), span.to(end)));
            }
            Tok::LBrace => {
                self.enter()?;
                let mut pairs = Vec::new();
                while self.peek().tok != Tok::RBrace {
                    let k = self.expr()?;
                    if self.peek().tok != Tok::Colon {
                        return Err(Error::syntax(
                            "sets are not part of the expression language; a dict takes `key: value`",
                            self.peek().span,
                        ));
                    }
                    self.next();
                    let v = self.expr()?;
                    pairs.push((k, v));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                let end = self.expect(Tok::RBrace, "`}`")?.span;
                self.depth -= 1;
                return Ok(Expr::new(ExprKind::Dict(pairs), span.to(end)));
            }
            Tok::End => return Err(Error::syntax("the expression ends too soon", span)),
            _ => return Err(Error::syntax("expected a value", span)),
        };
        Ok(Expr::new(kind, span))
    }
}

/// Operands joined by one of `and`/`or`: the operand itself when alone.
fn join(mut items: Vec<Expr>, kind: fn(Vec<Expr>) -> ExprKind) -> Expr {
    if items.len() == 1 {
        return items.pop().expect("one operand");
    }
    let span = items[0].span.to(items[items.len() - 1].span);
    Expr::new(kind(items), span)
}

/// A first operand and the operators and operands after it: the operand
/// itself when there are none.
fn chain(first: Expr, rest: Vec<(BinOp, Expr)>) -> Expr {
    if rest.is_empty() {
        return first;
    }
    let span = first.span.to(rest[rest.len() - 1].1.span);
    Expr::new(ExprKind::Chain(Box::new(first), rest), span)
}

pub fn float(v: f64) -> Value {
    Number::from_f64(v).map_or(Value::Null, Value::Number)
}
