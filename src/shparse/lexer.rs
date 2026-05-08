//! Tokenizer: source bytes → `Tok` stream. Quote-aware word accumulation.
//! Bails on any construct the classifier doesn't understand.

use super::cursor::Cursor;
use super::model::{Bail, RedirOp};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tok {
    Word(String),
    Assign { name: String, value: String },
    Semi,
    SemiSemi,
    And,          // &&
    Or,           // ||
    Amp,          // & (background)
    Pipe,         // |
    PipeBoth,     // |&
    Newline,
    Redir {
        fd: Option<u32>,
        op: RedirOp,
    },
}

pub fn tokenize(src: &str) -> Result<Vec<Tok>, Bail> {
    let mut c = Cursor::new(src);
    let mut out = Vec::new();
    let mut at_command_start = true;

    loop {
        skip_whitespace_and_comments(&mut c);
        let Some(b) = c.peek() else { break };

        match b {
            b'\n' => {
                c.bump();
                out.push(Tok::Newline);
                at_command_start = true;
            }
            b';' => {
                c.bump();
                if c.eat(b';') {
                    out.push(Tok::SemiSemi);
                } else {
                    out.push(Tok::Semi);
                }
                at_command_start = true;
            }
            b'&' => {
                c.bump();
                if c.eat(b'&') {
                    out.push(Tok::And);
                } else if c.eat(b'>') {
                    let op = if c.eat(b'>') { RedirOp::OutAllAppend } else { RedirOp::OutAll };
                    out.push(Tok::Redir { fd: None, op });
                } else {
                    out.push(Tok::Amp);
                }
                at_command_start = true;
            }
            b'|' => {
                c.bump();
                if c.eat(b'|') {
                    out.push(Tok::Or);
                } else if c.eat(b'&') {
                    out.push(Tok::PipeBoth);
                } else {
                    out.push(Tok::Pipe);
                }
                at_command_start = true;
            }
            b'(' => return Err(Bail::Subshell),
            b')' => return Err(Bail::Subshell),
            b'{' if starts_brace_group(&c) => return Err(Bail::BraceGroup),
            b'}' if at_command_start => return Err(Bail::BraceGroup),
            b'`' => return Err(Bail::CommandSubstitution),
            b'<' => {
                c.bump();
                if c.eat(b'<') {
                    if c.eat(b'<') {
                        return Err(Bail::HereString);
                    }
                    return Err(Bail::Heredoc);
                }
                if c.eat(b'(') {
                    return Err(Bail::ProcessSubstitution);
                }
                let op = if c.eat(b'&') { RedirOp::InDup } else { RedirOp::In };
                out.push(Tok::Redir { fd: None, op });
                at_command_start = false;
            }
            b'>' => {
                c.bump();
                if c.eat(b'(') {
                    return Err(Bail::ProcessSubstitution);
                }
                let op = if c.eat(b'>') {
                    RedirOp::OutAppend
                } else if c.eat(b'&') {
                    RedirOp::OutDup
                } else if c.eat(b'|') {
                    RedirOp::OutClobber
                } else {
                    RedirOp::OutTrunc
                };
                out.push(Tok::Redir { fd: None, op });
                at_command_start = false;
            }
            b'$' if c.peek_at(1) == Some(b'(') => {
                if c.peek_at(2) == Some(b'(') {
                    return Err(Bail::Arithmetic);
                }
                return Err(Bail::CommandSubstitution);
            }
            b'!' if at_command_start && is_boundary(c.peek_at(1)) => {
                // `!` as a command-position pipeline negation. Skip it; the
                // semantics of the wrapped command dictate safety.
                c.bump();
                continue;
            }
            b'[' if c.peek_at(1) == Some(b'[') => return Err(Bail::CompoundCommand),
            _ => {
                let (raw, decoded, has_unquoted_eq) = read_word(&mut c)?;
                // Reserved words at command start → compound command, bail.
                if at_command_start && !has_unquoted_eq && is_reserved_word(&decoded) {
                    return Err(Bail::CompoundCommand);
                }
                // Array literal: VAR=( ...
                if at_command_start && has_unquoted_eq && decoded.ends_with('=') {
                    // value is empty and next char is `(` → array literal
                    if c.peek() == Some(b'(') {
                        return Err(Bail::ArrayLiteral);
                    }
                }
                let tok = classify_word(&raw, &decoded, has_unquoted_eq, at_command_start);
                let is_assign = matches!(tok, Some(Tok::Assign { .. }));
                if let Some(tok) = tok {
                    out.push(tok);
                }
                // Stacked assignments `FOO=1 BAR=2 cmd` all classify as assigns.
                at_command_start = is_assign;
            }
        }
    }

    Ok(out)
}

/// A word is a concatenation of unquoted runs and quoted runs. Returns
/// (raw_bytes, decoded_string, had_unquoted_equals_in_name_position).
///
/// `decoded` collapses quotes: `a'b c'd` → `ab cd` (preserving spaces inside quotes).
/// `raw` preserves the original for cases that need exact source (e.g. assignments).
fn read_word(c: &mut Cursor) -> Result<(String, String, bool), Bail> {
    let mut raw = Vec::new();
    let mut decoded = Vec::new();
    let mut first_unquoted_eq: Option<usize> = None;

    while let Some(b) = c.peek() {
        match b {
            // Word terminators at top level
            b' ' | b'\t' | b'\n' | b';' | b'&' | b'|' | b'<' | b'>' | b'(' | b')' | b'`' => break,
            b'#' if raw.is_empty() => break,
            b'\\' => {
                c.bump();
                if let Some(next) = c.bump() {
                    if next == b'\n' {
                        // line continuation — consumes both chars, emits nothing
                        continue;
                    }
                    raw.push(b'\\');
                    raw.push(next);
                    decoded.push(next);
                }
            }
            b'\'' => {
                c.bump();
                raw.push(b'\'');
                loop {
                    match c.bump() {
                        None => return Err(Bail::UnterminatedQuote),
                        Some(b'\'') => {
                            raw.push(b'\'');
                            break;
                        }
                        Some(x) => {
                            raw.push(x);
                            decoded.push(x);
                        }
                    }
                }
            }
            b'"' => {
                c.bump();
                raw.push(b'"');
                loop {
                    match c.peek() {
                        None => return Err(Bail::UnterminatedQuote),
                        Some(b'"') => {
                            c.bump();
                            raw.push(b'"');
                            break;
                        }
                        Some(b'`') => return Err(Bail::CommandSubstitution),
                        Some(b'$') if c.peek_at(1) == Some(b'(') => {
                            return Err(if c.peek_at(2) == Some(b'(') {
                                Bail::Arithmetic
                            } else {
                                Bail::CommandSubstitution
                            });
                        }
                        Some(b'\\') => {
                            c.bump();
                            raw.push(b'\\');
                            let nxt = c.bump().ok_or(Bail::UnterminatedQuote)?;
                            raw.push(nxt);
                            // Inside "...", backslash only escapes $ ` " \ and newline.
                            // For classification we keep the escaped byte.
                            if nxt != b'\n' {
                                decoded.push(nxt);
                            }
                        }
                        Some(x) => {
                            c.bump();
                            raw.push(x);
                            decoded.push(x);
                        }
                    }
                }
            }
            b'$' if c.peek_at(1) == Some(b'\'') => {
                // ANSI-C quoting: $'...' — treat like single quotes but interpret \n \t etc.
                c.bump(); // $
                c.bump(); // '
                raw.extend_from_slice(b"$'");
                loop {
                    match c.bump() {
                        None => return Err(Bail::UnterminatedQuote),
                        Some(b'\'') => {
                            raw.push(b'\'');
                            break;
                        }
                        Some(b'\\') => {
                            raw.push(b'\\');
                            let nxt = c.bump().ok_or(Bail::UnterminatedQuote)?;
                            raw.push(nxt);
                            decoded.push(match nxt {
                                b'n' => b'\n',
                                b't' => b'\t',
                                b'r' => b'\r',
                                b'\\' => b'\\',
                                b'\'' => b'\'',
                                b'"' => b'"',
                                b'0' => 0,
                                other => other,
                            });
                        }
                        Some(x) => {
                            raw.push(x);
                            decoded.push(x);
                        }
                    }
                }
            }
            b'=' if first_unquoted_eq.is_none() && is_name_prefix(&raw) => {
                first_unquoted_eq = Some(raw.len());
                raw.push(b'=');
                decoded.push(b'=');
                c.bump();
            }
            x => {
                raw.push(x);
                decoded.push(x);
                c.bump();
            }
        }
    }

    let raw_s = String::from_utf8_lossy(&raw).into_owned();
    let dec_s = String::from_utf8_lossy(&decoded).into_owned();
    Ok((raw_s, dec_s, first_unquoted_eq.is_some()))
}

/// Classify a just-read word. If it's `NAME=value` at command-start (no argv yet
/// accumulated in the current segment), emit `Assign`. If it's a leading digit
/// sequence followed immediately by `<` or `>`, emit nothing and patch the next
/// redirect's fd. Otherwise return `Tok::Word`.
fn classify_word(
    raw: &str,
    decoded: &str,
    has_unquoted_eq: bool,
    at_command_start: bool,
) -> Option<Tok> {
    if at_command_start && has_unquoted_eq {
        if let Some(eq) = raw.find('=') {
            let name = &raw[..eq];
            if is_valid_name(name) {
                let value = decoded.get(eq + 1..).unwrap_or("").to_string();
                return Some(Tok::Assign { name: name.to_string(), value });
            }
        }
    }
    Some(Tok::Word(decoded.to_string()))
}

fn is_name_prefix(raw: &[u8]) -> bool {
    if raw.is_empty() {
        return false;
    }
    let first = raw[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    raw.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
}

fn is_valid_name(s: &str) -> bool {
    is_name_prefix(s.as_bytes())
}

fn is_boundary(b: Option<u8>) -> bool {
    matches!(b, None | Some(b' ') | Some(b'\t') | Some(b'\n'))
}

fn is_reserved_word(w: &str) -> bool {
    matches!(
        w,
        "if" | "then" | "else" | "elif" | "fi"
            | "while" | "until" | "do" | "done"
            | "for" | "in"
            | "case" | "esac"
            | "select" | "function" | "coproc"
            | "time" // we still handle as a wrapper, but at parse level
    ) && w != "time"
}

fn starts_brace_group(c: &Cursor) -> bool {
    // `{ ` or `{\n` or `{\t` indicates a command group; `{foo,bar}` is brace expansion
    // inside a word, which we treat opaquely.
    matches!(c.peek_at(1), Some(b' ') | Some(b'\t') | Some(b'\n'))
}

fn skip_whitespace_and_comments(c: &mut Cursor) {
    loop {
        // skip spaces/tabs and line continuations `\<newline>`
        let _ = c.eat_while(|b| b == b' ' || b == b'\t');
        if c.starts_with(b"\\\n") {
            c.bump();
            c.bump();
            continue;
        }
        if c.peek() == Some(b'#') {
            // comment to end of line
            c.eat_while(|b| b != b'\n');
            continue;
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shparse::model::RedirOp;

    fn lex(s: &str) -> Vec<Tok> {
        tokenize(s).expect("lex ok")
    }

    #[test]
    fn simple_command() {
        assert_eq!(lex("ls -la"), vec![
            Tok::Word("ls".into()),
            Tok::Word("-la".into()),
        ]);
    }

    #[test]
    fn operators() {
        assert_eq!(lex("a && b || c ; d | e |& f"), vec![
            Tok::Word("a".into()), Tok::And,
            Tok::Word("b".into()), Tok::Or,
            Tok::Word("c".into()), Tok::Semi,
            Tok::Word("d".into()), Tok::Pipe,
            Tok::Word("e".into()), Tok::PipeBoth,
            Tok::Word("f".into()),
        ]);
    }

    #[test]
    fn single_quotes_literal() {
        let toks = lex("echo 'a; b && c | d'");
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("a; b && c | d".into()),
        ]);
    }

    #[test]
    fn double_quotes_with_operators_inside() {
        let toks = lex(r#"echo "a; b""#);
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("a; b".into()),
        ]);
    }

    #[test]
    fn word_concatenation_mixed_quoting() {
        let toks = lex(r#"echo ab'cd ef'gh"ij kl"mn"#);
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("abcd efghij klmn".into()),
        ]);
    }

    #[test]
    fn backslash_escape() {
        let toks = lex(r"echo a\ b");
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("a b".into()),
        ]);
    }

    #[test]
    fn line_continuation() {
        let toks = lex("echo foo \\\nbar");
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("foo".into()),
            Tok::Word("bar".into()),
        ]);
    }

    #[test]
    fn comment_to_eol() {
        let toks = lex("echo foo # trailing");
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("foo".into()),
        ]);
    }

    #[test]
    fn assignments_before_command() {
        let toks = lex("FOO=bar BAZ=1 ls");
        assert_eq!(toks, vec![
            Tok::Assign { name: "FOO".into(), value: "bar".into() },
            Tok::Assign { name: "BAZ".into(), value: "1".into() },
            Tok::Word("ls".into()),
        ]);
    }

    #[test]
    fn assignment_after_command_is_word() {
        let toks = lex("ls FOO=bar");
        assert_eq!(toks, vec![
            Tok::Word("ls".into()),
            Tok::Word("FOO=bar".into()),
        ]);
    }

    #[test]
    fn redirects_basic() {
        let toks = lex("cmd > out < in 2>&1 >> app");
        let redirs: Vec<_> = toks.iter().filter_map(|t| match t {
            Tok::Redir { fd, op } => Some((*fd, *op)),
            _ => None,
        }).collect();
        assert_eq!(redirs, vec![
            (None, RedirOp::OutTrunc),
            (None, RedirOp::In),
            (None, RedirOp::OutDup),
            (None, RedirOp::OutAppend),
        ]);
    }

    #[test]
    fn bail_command_substitution() {
        assert_eq!(tokenize("echo $(whoami)"), Err(Bail::CommandSubstitution));
        assert_eq!(tokenize("echo `whoami`"), Err(Bail::CommandSubstitution));
    }

    #[test]
    fn bail_heredoc_and_herestring() {
        assert_eq!(tokenize("cat <<EOF"), Err(Bail::Heredoc));
        assert_eq!(tokenize("cat <<<str"), Err(Bail::HereString));
    }

    #[test]
    fn bail_process_substitution() {
        assert_eq!(tokenize("diff <(a) <(b)"), Err(Bail::ProcessSubstitution));
        assert_eq!(tokenize("tee >(log)"), Err(Bail::ProcessSubstitution));
    }

    #[test]
    fn bail_arithmetic() {
        assert_eq!(tokenize("echo $((1+2))"), Err(Bail::Arithmetic));
    }

    #[test]
    fn bail_subshell_and_group() {
        assert_eq!(tokenize("(ls)"), Err(Bail::Subshell));
        assert_eq!(tokenize("{ ls; }"), Err(Bail::BraceGroup));
    }

    #[test]
    fn ansi_c_quoting() {
        let toks = lex(r#"echo $'a\nb'"#);
        assert_eq!(toks, vec![
            Tok::Word("echo".into()),
            Tok::Word("a\nb".into()),
        ]);
    }

    #[test]
    fn semicolons_and_double_semi() {
        assert_eq!(lex("a ;; b"), vec![
            Tok::Word("a".into()),
            Tok::SemiSemi,
            Tok::Word("b".into()),
        ]);
    }
}
