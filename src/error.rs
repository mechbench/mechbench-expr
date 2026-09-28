//! What went wrong, and where in the expression.

use std::fmt;

/// A byte range in the expression's source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Span { start, end }
    }
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The text is not an expression of the language.
    Syntax,
    /// A value of the wrong type for what was done to it.
    Type,
    /// A name, function or method the language does not have.
    Name,
    /// An integer that left 64 bits, or an index past a list's end.
    Range,
    /// The evaluation ran out of its fuel or memory.
    Limit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub kind: Kind,
    pub message: String,
    pub span: Span,
}

impl Error {
    pub fn syntax(message: impl Into<String>, span: Span) -> Self {
        Error {
            kind: Kind::Syntax,
            message: message.into(),
            span,
        }
    }
    pub fn type_(message: impl Into<String>, span: Span) -> Self {
        Error {
            kind: Kind::Type,
            message: message.into(),
            span,
        }
    }
    pub fn name(message: impl Into<String>, span: Span) -> Self {
        Error {
            kind: Kind::Name,
            message: message.into(),
            span,
        }
    }
    pub fn range(message: impl Into<String>, span: Span) -> Self {
        Error {
            kind: Kind::Range,
            message: message.into(),
            span,
        }
    }
    pub fn limit(message: impl Into<String>, span: Span) -> Self {
        Error {
            kind: Kind::Limit,
            message: message.into(),
            span,
        }
    }
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            Kind::Syntax => "syntax",
            Kind::Type => "type",
            Kind::Name => "name",
            Kind::Range => "range",
            Kind::Limit => "limit",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (at {}..{})",
            self.message, self.span.start, self.span.end
        )
    }
}
