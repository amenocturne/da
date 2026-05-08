//! Token stream → `Vec<Segment>`. Each segment is one command with its
//! arguments, assignments, and attached redirects; the `follows` field says
//! which operator separates it from the next segment (or `End`).

use super::lexer::{tokenize, Tok};
use super::model::{Bail, RedirOp, Redirect, Segment, Separator};

pub fn parse(src: &str) -> Result<Vec<Segment>, Bail> {
    let toks = tokenize(src)?;
    segments(&toks)
}

pub fn segments(toks: &[Tok]) -> Result<Vec<Segment>, Bail> {
    let mut out = Vec::new();
    let mut cur = Segment {
        assigns: Vec::new(),
        argv: Vec::new(),
        redirects: Vec::new(),
        follows: Separator::End,
    };
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Newline | Tok::Semi => {
                finish(&mut out, &mut cur, Separator::Semi);
                i += 1;
            }
            Tok::SemiSemi => {
                // ;; is a case-pattern terminator; at top level we treat it
                // as close enough to `;` to just end the segment.
                finish(&mut out, &mut cur, Separator::Semi);
                i += 1;
            }
            Tok::Amp => {
                // background — classify like ;. An unsupervised `&` doesn't change
                // safety of the preceding command.
                finish(&mut out, &mut cur, Separator::Semi);
                i += 1;
            }
            Tok::And => {
                finish(&mut out, &mut cur, Separator::And);
                i += 1;
            }
            Tok::Or => {
                finish(&mut out, &mut cur, Separator::Or);
                i += 1;
            }
            Tok::Pipe => {
                finish(&mut out, &mut cur, Separator::Pipe);
                i += 1;
            }
            Tok::PipeBoth => {
                finish(&mut out, &mut cur, Separator::PipeBoth);
                i += 1;
            }
            Tok::Assign { name, value } => {
                if cur.argv.is_empty() {
                    cur.assigns.push((name.clone(), value.clone()));
                } else {
                    // argv has started — assignments after the command are just words
                    cur.argv.push(format!("{name}={value}"));
                }
                i += 1;
            }
            Tok::Word(w) => {
                // digit-prefix fd: `2> foo` or `2>&1` — bash allows an optional
                // unbroken digit run immediately before a redirect op, with no
                // whitespace between. The lexer already separated `2` into a
                // Word (since it was followed by `>` which terminates words).
                // We detect this here by checking if the next token is a Redir
                // with no fd.
                if let Some(Tok::Redir { fd, op }) = toks.get(i + 1) {
                    if fd.is_none() {
                        if let Ok(n) = w.parse::<u32>() {
                            // Only treat as fd prefix if there was no space between
                            // `w` and the redirect. We can't know positions from
                            // Tok alone — approximate by always attaching an
                            // all-digit word immediately preceding a fd-less
                            // Redir as the fd. This is what bash does anyway.
                            let op = *op;
                            i += 2;
                            let target = read_redir_target(toks, &mut i, op)?;
                            cur.redirects.push(Redirect { fd: Some(n), op, target });
                            continue;
                        }
                    }
                }
                cur.argv.push(w.clone());
                i += 1;
            }
            Tok::Redir { fd, op } => {
                let op = *op;
                let fd = *fd;
                i += 1;
                let target = read_redir_target(toks, &mut i, op)?;
                cur.redirects.push(Redirect { fd, op, target });
            }
        }
    }
    if !cur.is_empty() {
        cur.follows = Separator::End;
        out.push(cur);
    }
    Ok(out)
}

fn read_redir_target(toks: &[Tok], i: &mut usize, op: RedirOp) -> Result<String, Bail> {
    // Redirect target is the next Word. For OutDup/InDup, the "word" is typically
    // a digit or `-`; we just take whatever word comes next.
    let _ = op;
    match toks.get(*i) {
        Some(Tok::Word(w)) => {
            *i += 1;
            Ok(w.clone())
        }
        _ => Err(Bail::UnexpectedToken),
    }
}

fn finish(out: &mut Vec<Segment>, cur: &mut Segment, sep: Separator) {
    if cur.is_empty() {
        return;
    }
    cur.follows = sep;
    out.push(std::mem::replace(cur, Segment {
        assigns: Vec::new(),
        argv: Vec::new(),
        redirects: Vec::new(),
        follows: Separator::End,
    }));
}

impl Segment {
    fn is_empty(&self) -> bool {
        self.assigns.is_empty() && self.argv.is_empty() && self.redirects.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(cmd: &str) -> Vec<Segment> {
        parse(cmd).expect("parse ok")
    }

    #[test]
    fn one_simple() {
        let s = seg("ls -la");
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].argv, vec!["ls", "-la"]);
        assert!(s[0].redirects.is_empty());
        assert!(s[0].assigns.is_empty());
        assert_eq!(s[0].follows, Separator::End);
    }

    #[test]
    fn pipeline_three() {
        let s = seg("a | b | c");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].follows, Separator::Pipe);
        assert_eq!(s[1].follows, Separator::Pipe);
        assert_eq!(s[2].follows, Separator::End);
    }

    #[test]
    fn mixed_ops() {
        let s = seg("a && b; c || d | e");
        let follows: Vec<_> = s.iter().map(|x| x.follows).collect();
        assert_eq!(follows, vec![
            Separator::And,
            Separator::Semi,
            Separator::Or,
            Separator::Pipe,
            Separator::End,
        ]);
    }

    #[test]
    fn assignments_and_argv() {
        let s = seg("FOO=bar BAZ=1 ls -la");
        assert_eq!(s[0].assigns, vec![
            ("FOO".into(), "bar".into()),
            ("BAZ".into(), "1".into()),
        ]);
        assert_eq!(s[0].argv, vec!["ls", "-la"]);
    }

    #[test]
    fn redirects_with_and_without_fd() {
        let s = seg("cmd 2>/dev/null 1>&2 > out <in");
        let r = &s[0].redirects;
        assert_eq!(r[0].fd, Some(2));
        assert_eq!(r[0].op, RedirOp::OutTrunc);
        assert_eq!(r[0].target, "/dev/null");
        assert_eq!(r[1].fd, Some(1));
        assert_eq!(r[1].op, RedirOp::OutDup);
        assert_eq!(r[1].target, "2");
        assert_eq!(r[2].fd, None);
        assert_eq!(r[2].op, RedirOp::OutTrunc);
        assert_eq!(r[2].target, "out");
        assert_eq!(r[3].fd, None);
        assert_eq!(r[3].op, RedirOp::In);
        assert_eq!(r[3].target, "in");
    }

    #[test]
    fn stderr_to_stdout_dup() {
        let s = seg("cmd 2>&1");
        assert_eq!(s[0].redirects[0].fd, Some(2));
        assert_eq!(s[0].redirects[0].op, RedirOp::OutDup);
        assert_eq!(s[0].redirects[0].target, "1");
    }

    #[test]
    fn compound_with_redirect_before_separator() {
        let s = seg("find . 2>/dev/null; stat x 2>&1 | head -5");
        assert_eq!(s.len(), 3);
        assert_eq!(s[0].argv, vec!["find", "."]);
        assert_eq!(s[0].redirects[0].target, "/dev/null");
        assert_eq!(s[0].follows, Separator::Semi);
        assert_eq!(s[1].argv, vec!["stat", "x"]);
        assert_eq!(s[1].redirects[0].target, "1");
        assert_eq!(s[1].follows, Separator::Pipe);
        assert_eq!(s[2].argv, vec!["head", "-5"]);
    }

    #[test]
    fn amp_background_ends_segment() {
        let s = seg("a & b");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].argv, vec!["a"]);
        assert_eq!(s[1].argv, vec!["b"]);
    }
}
