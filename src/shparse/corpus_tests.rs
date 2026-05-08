//! Differential tests against the Parable corpus (vendored in tests/corpus/).
//!
//! Strategy: for each case, look at the input bash source and independently
//! decide whether the parser *should* bail (contains out-of-scope constructs
//! like $(...), heredocs, compound commands, etc.). Then compare to what our
//! parser actually does. No attempt to match Parable's AST — we only care
//! that bail decisions are consistent.

use super::parser::parse;

const IN_SCOPE: &[(&str, &str)] = &[
    ("words", include_str!("../../tests/corpus/01_words.tests")),
    ("commands", include_str!("../../tests/corpus/02_commands.tests")),
    ("pipelines", include_str!("../../tests/corpus/03_pipelines.tests")),
    ("lists", include_str!("../../tests/corpus/04_lists.tests")),
    ("redirects", include_str!("../../tests/corpus/05_redirects.tests")),
    ("negation_time", include_str!("../../tests/corpus/16_negation_time.tests")),
    ("pipe_stderr", include_str!("../../tests/corpus/22_pipe_stderr.tests")),
    ("ansi_c_quoting", include_str!("../../tests/corpus/24_ansi_c_quoting.tests")),
    ("line_continuation", include_str!("../../tests/corpus/34_line_continuation.tests")),
];

const BAIL_EXPECTED: &[(&str, &str)] = &[
    ("command_substitution", include_str!("../../tests/corpus/12_command_substitution.tests")),
    ("arithmetic", include_str!("../../tests/corpus/13_arithmetic.tests")),
    ("here_documents", include_str!("../../tests/corpus/14_here_documents.tests")),
    ("process_substitution", include_str!("../../tests/corpus/15_process_substitution.tests")),
];

/// Inspect the raw input bash and decide whether a correct parser must
/// return `Err(Bail::*)`. Respects single-quote state (nothing inside
/// `'...'` triggers anything). Inside double quotes: `$( ` and backticks
/// still do.
fn expect_bail(src: &str) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;

    while i < n {
        let b = bytes[i];
        if in_single {
            if b == b'\'' { in_single = false; }
            i += 1;
            continue;
        }
        if b == b'\\' {
            i += 2;
            continue;
        }
        if b == b'\'' && !in_double {
            in_single = true;
            i += 1;
            continue;
        }
        if b == b'"' {
            in_double = !in_double;
            i += 1;
            continue;
        }
        // Things that trigger bail regardless of double-quote state:
        if b == b'$' && i + 1 < n && bytes[i + 1] == b'(' {
            return true;
        }
        if b == b'`' {
            return true;
        }
        // Things that trigger only outside quotes:
        if !in_double {
            if b == b'<' && i + 1 < n && (bytes[i + 1] == b'<' || bytes[i + 1] == b'(') {
                return true;
            }
            if b == b'>' && i + 1 < n && bytes[i + 1] == b'(' {
                return true;
            }
        }
        i += 1;
    }

    // Any unquoted `(` that isn't `$(`, `<(`, `>(`, or the start of `((`.
    // Captures subshells, function defs (`f()`), and arithmetic commands.
    if has_unquoted_subshell_paren(src) {
        return true;
    }
    // Any unquoted `{ ` — brace group.
    if contains_unquoted(src, b"{ ") || contains_unquoted(src, b"{\t") {
        return true;
    }
    // Reserved word at any segment boundary.
    if has_reserved_word_at_segment_start(src) {
        return true;
    }
    // Array literal: unquoted `=(`.
    if contains_unquoted(src, b"=(") {
        return true;
    }
    // `[[` conditional expression.
    if contains_unquoted(src, b"[[") {
        return true;
    }
    false
}

/// `(` that starts a subshell, function def, or arithmetic command.
/// Ignores `$(`, `<(`, `>(`, and the second `(` of `((`.
fn has_unquoted_subshell_paren(src: &str) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut in_single = false;
    let mut in_double = false;
    while i < n {
        let b = bytes[i];
        if b == b'\\' { i += 2; continue; }
        if b == b'\'' && !in_double { in_single = !in_single; i += 1; continue; }
        if b == b'"' && !in_single { in_double = !in_double; i += 1; continue; }
        if !in_single && !in_double && b == b'(' {
            let prev = if i > 0 { bytes[i - 1] } else { 0 };
            if prev != b'$' && prev != b'<' && prev != b'>' && prev != b'(' {
                return true;
            }
        }
        i += 1;
    }
    false
}

fn has_reserved_word_at_segment_start(src: &str) -> bool {
    // Walk the source, tracking quote state, and at every "command start"
    // position (start of input, or after `;` / `|` / `&&` / `||` / `&`)
    // check if the next word is a reserved keyword.
    let bytes = src.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    let mut at_start = true;
    let mut in_single = false;
    let mut in_double = false;
    while i < n {
        let b = bytes[i];
        if in_single {
            if b == b'\'' { in_single = false; }
            i += 1;
            continue;
        }
        if b == b'\\' { i += 2; continue; }
        if b == b'\'' && !in_double { in_single = true; i += 1; continue; }
        if b == b'"' { in_double = !in_double; i += 1; continue; }
        if in_double { i += 1; continue; }

        if b == b' ' || b == b'\t' || b == b'\n' { i += 1; continue; }
        if b == b';' || b == b'|' || b == b'&' {
            at_start = true;
            // Skip doubled operators
            if i + 1 < n && (bytes[i + 1] == b'&' || bytes[i + 1] == b'|' || bytes[i + 1] == b';') {
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if at_start {
            // Read the next word (unquoted alphanumeric-ish)
            let start = i;
            while i < n {
                let x = bytes[i];
                if x == b' ' || x == b'\t' || x == b'\n' || x == b';' || x == b'|'
                    || x == b'&' || x == b'<' || x == b'>' || x == b'(' || x == b')'
                    || x == b'\'' || x == b'"' || x == b'='
                {
                    break;
                }
                i += 1;
            }
            let word = &src[start..i];
            if matches!(
                word,
                "if" | "then" | "else" | "elif" | "fi"
                    | "while" | "until" | "do" | "done"
                    | "for" | "in"
                    | "case" | "esac"
                    | "select" | "function" | "coproc"
            ) {
                return true;
            }
            at_start = false;
            continue;
        }
        i += 1;
    }
    false
}

fn contains_unquoted(src: &str, needle: &[u8]) -> bool {
    let bytes = src.as_bytes();
    let n = bytes.len();
    let k = needle.len();
    let mut in_single = false;
    let mut in_double = false;
    let mut i = 0;
    while i + k <= n {
        let b = bytes[i];
        if b == b'\\' { i += 2; continue; }
        if b == b'\'' && !in_double { in_single = !in_single; i += 1; continue; }
        if b == b'"' && !in_single { in_double = !in_double; i += 1; continue; }
        if !in_single && !in_double && &bytes[i..i + k] == needle {
            return true;
        }
        i += 1;
    }
    false
}

#[derive(Debug)]
struct Case {
    name: String,
    input: String,
}

fn parse_cases(src: &str) -> Vec<Case> {
    let mut out = Vec::new();
    let mut lines = src.lines();
    while let Some(line) = lines.next() {
        if let Some(name) = line.strip_prefix("=== ") {
            let mut input_buf = String::new();
            for l in lines.by_ref() {
                if l == "---" { break; }
                if !input_buf.is_empty() { input_buf.push('\n'); }
                input_buf.push_str(l);
            }
            // skip the expected s-exp block
            for l in lines.by_ref() {
                if l == "---" { break; }
            }
            out.push(Case { name: name.trim().to_string(), input: input_buf });
        }
    }
    out
}

#[test]
fn corpus_differential() {
    let mut total = 0;
    let mut ok = 0;
    let mut failures: Vec<String> = Vec::new();

    let all = IN_SCOPE.iter().chain(BAIL_EXPECTED.iter());
    for (label, contents) in all {
        let cases = parse_cases(contents);
        for c in &cases {
            total += 1;
            let expected_bail = expect_bail(&c.input);
            let got = parse(&c.input);
            match (got, expected_bail) {
                (Err(_), true) => ok += 1,
                (Ok(_), false) => ok += 1,
                (Ok(segs), true) => {
                    failures.push(format!(
                        "[{label}/{name}] expected bail, got Ok({segs} segs) for {input:?}",
                        name = c.name, segs = segs.len(), input = c.input
                    ));
                }
                (Err(e), false) => {
                    failures.push(format!(
                        "[{label}/{name}] expected Ok, got {e:?} for {input:?}",
                        name = c.name, e = e, input = c.input
                    ));
                }
            }
        }
    }

    eprintln!("corpus: {ok}/{total} passed");
    for f in failures.iter().take(40) {
        eprintln!("FAIL {f}");
    }
    if !failures.is_empty() {
        panic!("corpus differential: {} failures out of {}", failures.len(), total);
    }
}
