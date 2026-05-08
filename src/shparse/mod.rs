//! Minimal bash parser for `dabin`'s classification needs.
//! See `model::Bail` for the constructs that intentionally short-circuit.

pub mod cursor;
pub mod lexer;
pub mod model;
pub mod parser;

#[cfg(test)]
mod corpus_tests;

pub use model::{Bail, RedirOp, Redirect, Segment, Separator};
pub use parser::parse;
