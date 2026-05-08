//! AST the classifier consumes. Minimal on purpose: everything we don't
//! need to classify safely either stays inside a `Word` (as opaque bytes) or
//! causes the lexer to return `Bail`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub assigns: Vec<(String, String)>,
    pub argv: Vec<String>,
    pub redirects: Vec<Redirect>,
    pub follows: Separator,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Separator {
    End,
    Semi,
    And,
    Or,
    Pipe,
    PipeBoth,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirect {
    pub fd: Option<u32>,
    pub op: RedirOp,
    pub target: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirOp {
    /// `>`  write, truncate
    OutTrunc,
    /// `>>` write, append
    OutAppend,
    /// `<`  read
    In,
    /// `>&` dup output fd (target is a digit or `-`)
    OutDup,
    /// `<&` dup input fd
    InDup,
    /// `&>` stdout+stderr to file (truncate)
    OutAll,
    /// `&>>` stdout+stderr to file (append)
    OutAllAppend,
    /// `>|` force truncate (ignore noclobber)
    OutClobber,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bail {
    CommandSubstitution,
    ProcessSubstitution,
    Heredoc,
    HereString,
    Arithmetic,
    Subshell,
    BraceGroup,
    /// Compound command reserved word at command position: if/while/for/case/...
    CompoundCommand,
    /// Array literal: VAR=( ... )
    ArrayLiteral,
    UnterminatedQuote,
    UnexpectedToken,
}
