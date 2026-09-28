//! Tokens of the expression language: Python's, for the productions the
//! language keeps.

use crate::error::{Error, Span};

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Float(f64),
    Str(String),
    Name(String),
    // Keywords.
    And,
    Or,
    Not,
    In,
    Is,
    If,
    Else,
    For,
    True,
    False,
    None,
    // Punctuation and operators.
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    Plus,
    Minus,
    Star,
    StarStar,
    Slash,
    SlashSlash,
    Percent,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Assign,
    End,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

/// Words the language reserves: Python's keywords it keeps, and those it
/// refuses by name so a reader is told why.
const REFUSED: &[&str] = &[
    "lambda", "def", "class", "import", "from", "global", "nonlocal", "del", "pass", "return",
    "yield", "await", "async", "while", "with", "try", "except", "finally", "raise", "assert",
    "break", "continue", "as", "elif",
];

pub fn lex(src: &str) -> Result<Vec<Token>, Error> {
    let chars: Vec<(usize, char)> = src.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let at = |i: usize| chars.get(i).map(|c| c.1);
    let pos = |i: usize| chars.get(i).map_or(src.len(), |c| c.0);
    while i < chars.len() {
        let c = chars[i].1;
        let start = pos(i);
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || (c == '.' && at(i + 1).is_some_and(|d| d.is_ascii_digit())) {
            let (tok, next) = number(&chars, i, src)?;
            out.push(Token {
                tok,
                span: Span::new(start, pos(next)),
            });
            i = next;
            continue;
        }
        if c == '"' || c == '\'' {
            let (s, next) = string(&chars, i, src)?;
            out.push(Token {
                tok: Tok::Str(s),
                span: Span::new(start, pos(next)),
            });
            i = next;
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let mut j = i;
            while j < chars.len() && (chars[j].1.is_alphanumeric() || chars[j].1 == '_') {
                j += 1;
            }
            let word = &src[start..pos(j)];
            let span = Span::new(start, pos(j));
            if REFUSED.contains(&word) {
                return Err(Error::syntax(
                    format!("`{word}` is not part of the expression language"),
                    span,
                ));
            }
            let tok = match word {
                "and" => Tok::And,
                "or" => Tok::Or,
                "not" => Tok::Not,
                "in" => Tok::In,
                "is" => Tok::Is,
                "if" => Tok::If,
                "else" => Tok::Else,
                "for" => Tok::For,
                "True" | "true" => Tok::True,
                "False" | "false" => Tok::False,
                "None" | "null" => Tok::None,
                _ => Tok::Name(word.to_string()),
            };
            out.push(Token { tok, span });
            i = j;
            continue;
        }
        let two: String = chars[i..chars.len().min(i + 2)]
            .iter()
            .map(|c| c.1)
            .collect();
        let (tok, len) = match two.as_str() {
            "**" => (Tok::StarStar, 2),
            "//" => (Tok::SlashSlash, 2),
            "==" => (Tok::Eq, 2),
            "!=" => (Tok::Ne, 2),
            "<=" => (Tok::Le, 2),
            ">=" => (Tok::Ge, 2),
            ":=" => {
                return Err(Error::syntax(
                    "assignment is not part of the expression language",
                    Span::new(start, start + 2),
                ));
            }
            _ => match c {
                '(' => (Tok::LParen, 1),
                ')' => (Tok::RParen, 1),
                '[' => (Tok::LBracket, 1),
                ']' => (Tok::RBracket, 1),
                '{' => (Tok::LBrace, 1),
                '}' => (Tok::RBrace, 1),
                ',' => (Tok::Comma, 1),
                ':' => (Tok::Colon, 1),
                '.' => (Tok::Dot, 1),
                '+' => (Tok::Plus, 1),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                '%' => (Tok::Percent, 1),
                '<' => (Tok::Lt, 1),
                '>' => (Tok::Gt, 1),
                '=' => (Tok::Assign, 1),
                _ => {
                    return Err(Error::syntax(
                        format!("unexpected character `{c}`"),
                        Span::new(start, start + c.len_utf8()),
                    ));
                }
            },
        };
        out.push(Token {
            tok,
            span: Span::new(start, pos(i + len)),
        });
        i += len;
    }
    out.push(Token {
        tok: Tok::End,
        span: Span::new(src.len(), src.len()),
    });
    Ok(out)
}

fn number(chars: &[(usize, char)], mut i: usize, src: &str) -> Result<(Tok, usize), Error> {
    let start_i = i;
    let pos = |i: usize| chars.get(i).map_or(src.len(), |c| c.0);
    let mut float = false;
    let digits = |i: &mut usize| {
        while *i < chars.len() && (chars[*i].1.is_ascii_digit() || chars[*i].1 == '_') {
            *i += 1;
        }
    };
    digits(&mut i);
    if i < chars.len() && chars[i].1 == '.' {
        float = true;
        i += 1;
        digits(&mut i);
    }
    if i < chars.len() && (chars[i].1 == 'e' || chars[i].1 == 'E') {
        let mut j = i + 1;
        if j < chars.len() && (chars[j].1 == '+' || chars[j].1 == '-') {
            j += 1;
        }
        if j < chars.len() && chars[j].1.is_ascii_digit() {
            float = true;
            i = j;
            digits(&mut i);
        }
    }
    let span = Span::new(pos(start_i), pos(i));
    let text: String = src[span.start..span.end]
        .chars()
        .filter(|c| *c != '_')
        .collect();
    if float {
        let v: f64 = text
            .parse()
            .map_err(|_| Error::syntax(format!("`{text}` is not a number"), span))?;
        if !v.is_finite() {
            return Err(Error::syntax(
                format!("`{text}` is too large for a number"),
                span,
            ));
        }
        Ok((Tok::Float(v), i))
    } else {
        let v: i64 = text.parse().map_err(|_| {
            Error::syntax(
                format!("`{text}` is larger than an integer holds (64 bits)"),
                span,
            )
        })?;
        Ok((Tok::Int(v), i))
    }
}

fn string(chars: &[(usize, char)], i: usize, src: &str) -> Result<(String, usize), Error> {
    let quote = chars[i].1;
    let pos = |i: usize| chars.get(i).map_or(src.len(), |c| c.0);
    let mut out = String::new();
    let mut j = i + 1;
    while j < chars.len() {
        let c = chars[j].1;
        if c == quote {
            return Ok((out, j + 1));
        }
        if c == '\n' {
            break;
        }
        if c == '\\' {
            let e = chars.get(j + 1).map(|c| c.1);
            let esc = match e {
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some('\\') => '\\',
                Some('\'') => '\'',
                Some('"') => '"',
                Some('0') => '\0',
                Some('u') => {
                    let hex: String = chars
                        .get(j + 2..j + 6)
                        .map(|s| s.iter().map(|c| c.1).collect())
                        .unwrap_or_default();
                    let code = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32);
                    match code {
                        Some(ch) if hex.len() == 4 => {
                            out.push(ch);
                            j += 6;
                            continue;
                        }
                        _ => {
                            return Err(Error::syntax(
                                "`\\u` takes four hex digits",
                                Span::new(pos(j), pos(j + 2)),
                            ));
                        }
                    }
                }
                _ => {
                    return Err(Error::syntax(
                        "unknown escape in a string",
                        Span::new(pos(j), pos(j + 2)),
                    ));
                }
            };
            out.push(esc);
            j += 2;
            continue;
        }
        out.push(c);
        j += 1;
    }
    Err(Error::syntax(
        "a string is not closed",
        Span::new(pos(i), pos(j)),
    ))
}
