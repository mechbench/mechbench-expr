//! The mechbench expression language (`SPEC.md`):
//! a strict subset of Python's expressions with total, deterministic
//! semantics, one engine for compute, the API and the browser.

pub mod api;
pub mod ast;
pub mod canon;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod library;
pub mod parser;
pub mod pyfmt;
pub mod template;
pub mod value;

pub use api::handle;
pub use error::Error;
pub use parser::parse;
