//! `dabin` (binary `da`) — classify a bash command as approve/defer/deny
//! under an explicitly-named set of policies.
//!
//! The library is the engine; the binary is a thin CLI wrapper. Embedders
//! who want to compose their own classification pipeline depend on this
//! crate directly and use [`classify`] with whichever [`Policy`] values
//! they like (built-ins from [`policies`] or their own).

pub mod classifier;
pub mod policies;
pub mod shparse;

mod engine;

pub use engine::{classify, classify_with_ml};
pub use shparse::{parse, Bail, RedirOp, Redirect, Segment, Separator};

use std::path::Path;

/// What a single [`Policy`] says about a single segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// This policy vouches for the segment.
    Approve,
    /// This policy actively rejects the segment.
    Deny,
}

/// The engine's final answer for a whole command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Every segment was approved by some policy (or by the engine's
    /// structural rules — `cd`, bare assignments, `env`/`time` wrappers).
    Approve,
    /// At least one segment was actively denied by a policy. Callers may
    /// choose to surface this as a hard rejection.
    Deny,
    /// No policy spoke up for at least one segment, or the parser bailed.
    /// Callers should fall through to whatever default exists (e.g. a
    /// permission prompt).
    Defer,
}

/// A single policy. Atomic: each value covers exactly one capability.
/// Adding a new capability is one new value with its own [`verify`] fn —
/// no central registry to update.
///
/// [`verify`]: Policy::verify
#[derive(Debug, Clone, Copy)]
pub struct Policy {
    /// Stable string id (e.g. `"git:read"`). Used in error messages and
    /// for diagnostics; the CLI maps `--git read` to this name internally.
    pub name: &'static str,
    /// Inspect a single segment. Return [`Some(Verdict::Approve)`] to
    /// vouch for the segment, [`Some(Verdict::Deny)`] to actively reject,
    /// or [`None`] to abstain (let later policies have a say).
    pub verify: fn(seg: &Segment, path: Option<&Path>) -> Option<Verdict>,
}
