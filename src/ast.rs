//! The expression tree.

use crate::error::Span;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    FloorDiv,
    Mod,
    Pow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    In,
    NotIn,
    Is,
    IsNot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Pos,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompKind {
    /// `[x for x in xs]`: a list.
    List,
    /// `sum(x for x in xs)`: the values, handed to the call around it.
    Generator,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Arg {
    pub name: Option<String>,
    pub value: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExprKind {
    Lit(Value),
    Name(String),
    Attr(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    Slice(
        Box<Expr>,
        Option<Box<Expr>>,
        Option<Box<Expr>>,
        Option<Box<Expr>>,
    ),
    Call(String, Vec<Arg>),
    Method(Box<Expr>, String, Vec<Arg>),
    Unary(UnOp, Box<Expr>),
    /// `a ** b`: the one binary operator kept as a pair, since it groups
    /// right to left.
    Binary(BinOp, Box<Expr>, Box<Expr>),
    /// A chain of operators at one level, `a + b - c` or `a * b / c`, as
    /// one node read left to right: a sum of a thousand terms is one level
    /// deep, parsed and evaluated in a loop.
    Chain(Box<Expr>, Vec<(BinOp, Expr)>),
    Compare(Box<Expr>, Vec<(CmpOp, Expr)>),
    /// `a and b and c`: its operands, at least two, one level deep.
    And(Vec<Expr>),
    /// `a or b or c`: likewise.
    Or(Vec<Expr>),
    IfElse {
        then: Box<Expr>,
        cond: Box<Expr>,
        otherwise: Box<Expr>,
    },
    List(Vec<Expr>),
    Dict(Vec<(Expr, Expr)>),
    Comp {
        kind: CompKind,
        elt: Box<Expr>,
        var: String,
        iter: Box<Expr>,
        cond: Option<Box<Expr>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

impl Expr {
    pub fn new(kind: ExprKind, span: Span) -> Self {
        Expr { kind, span }
    }
}
