//! `da` — read a bash command on stdin, classify it under the policies
//! enabled by flags, exit 0/1/2/64 (approve/defer/deny/usage error).
//!
//! No JSON, no defaults, no implicit anything. Wrappers (e.g. for Claude
//! Code's hook protocol) live with the caller.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;

use dabin::policies::{self, HELP_BYPASS, MACOS_ONLY, MKDIR_CWD, READ_ONLY};
use dabin::{classify_with_ml, Decision, Policy, Segment, Verdict};

const EXIT_APPROVE: u8 = 0;
const EXIT_DEFER: u8 = 1;
const EXIT_DENY: u8 = 2;
const EXIT_USAGE: u8 = 64;

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(msg) => {
            eprintln!("da: {msg}");
            print_usage();
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn run() -> Result<u8, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let parsed = parse_args(&args)?;

    let mut cmd = String::new();
    std::io::stdin()
        .read_to_string(&mut cmd)
        .map_err(|e| format!("reading stdin: {e}"))?;
    // Trim a single trailing newline (common with `printf '%s\n'` callers).
    let cmd = cmd.trim_end_matches('\n');

    let decision = classify_with_ml(cmd, parsed.path.as_deref(), &parsed.policies);
    Ok(match decision {
        Decision::Approve => EXIT_APPROVE,
        Decision::Deny if parsed.autonomous => EXIT_DENY,
        Decision::Defer if parsed.autonomous => EXIT_DENY,
        Decision::Deny | Decision::Defer => EXIT_DEFER,
    })
}

struct Parsed {
    path: Option<PathBuf>,
    policies: Vec<&'static Policy>,
    autonomous: bool,
}

fn parse_args(args: &[String]) -> Result<Parsed, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_usage();
        // Help is shown on stdout *and* we exit zero — but `run()` only
        // returns from the success path, so we exit here directly.
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("da {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }

    let mut path: Option<PathBuf> = None;
    let mut policies: Vec<&'static Policy> = Vec::new();
    let mut custom_rules: Vec<Vec<String>> = Vec::new();
    let mut deny_patterns: Vec<String> = Vec::new();
    let mut autonomous = false;
    let mut i = 0;

    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--path" => {
                let v = args.get(i + 1)
                    .ok_or_else(|| "--path requires a value".to_string())?;
                path = Some(PathBuf::from(v));
                i += 2;
            }
            "--read-only" => {
                policies.push(&READ_ONLY);
                i += 1;
            }
            "--macos-only" => {
                policies.push(&MACOS_ONLY);
                i += 1;
            }
            "--help-bypass" => {
                policies.push(&HELP_BYPASS);
                i += 1;
            }
            "--mkdir-cwd" => {
                policies.push(&MKDIR_CWD);
                i += 1;
            }
            "--git" => {
                let v = args.get(i + 1)
                    .ok_or_else(|| "--git requires a value".to_string())?;
                add_capabilities(&mut policies, "git", v)?;
                i += 2;
            }
            "--cargo" => {
                let v = args.get(i + 1)
                    .ok_or_else(|| "--cargo requires a value".to_string())?;
                add_capabilities(&mut policies, "cargo", v)?;
                i += 2;
            }
            "--autonomous" => {
                autonomous = true;
                i += 1;
            }
            "--interactive" => {
                i += 1;
            }
            "--allow" => {
                let v = args.get(i + 1)
                    .ok_or_else(|| "--allow requires a value".to_string())?;
                let tokens: Vec<String> = v.split_whitespace().map(str::to_owned).collect();
                if tokens.is_empty() {
                    return Err("--allow value is empty".into());
                }
                custom_rules.push(tokens);
                i += 2;
            }
            "--deny" => {
                let v = args.get(i + 1)
                    .ok_or_else(|| "--deny requires a value".to_string())?;
                if v.is_empty() {
                    return Err("--deny value is empty".into());
                }
                deny_patterns.push(v.clone());
                i += 2;
            }
            other => return Err(format!("unknown flag: {other}")),
        }
    }

    if !custom_rules.is_empty() {
        CUSTOM_ALLOW_RULES
            .set(custom_rules)
            .map_err(|_| "internal: custom rules already initialised".to_string())?;
        policies.push(&CUSTOM_ALLOW);
    }

    // Deny policies go first — they override allow policies.
    if !deny_patterns.is_empty() {
        CUSTOM_DENY_PATTERNS
            .set(deny_patterns)
            .map_err(|_| "internal: deny patterns already initialised".to_string())?;
        policies.insert(0, &CUSTOM_DENY);
    }

    if policies.is_empty() {
        return Err("no policies enabled — pass at least one policy flag".into());
    }
    Ok(Parsed { path, policies, autonomous })
}

/// User-supplied token-prefix rules from `--allow`. Set once during arg
/// parsing; read by [`custom_allow_verify`]. Each inner `Vec<String>` is a
/// prefix of literal argv tokens — a segment approves if its argv starts
/// with the same tokens.
static CUSTOM_ALLOW_RULES: OnceLock<Vec<Vec<String>>> = OnceLock::new();

/// Policy backing `--allow`. Stateless function pointer + global rule
/// storage; the storage is process-local and set once before classify runs.
static CUSTOM_ALLOW: Policy = Policy {
    name: "custom-allow",
    verify: custom_allow_verify,
};

/// User-supplied deny patterns from `--deny`. Set once during arg parsing;
/// read by [`custom_deny_verify`]. Each string is a path component pattern —
/// a segment denies if any argv token or redirect target contains a matching
/// path component. Trailing `*` enables prefix matching.
static CUSTOM_DENY_PATTERNS: OnceLock<Vec<String>> = OnceLock::new();

static CUSTOM_DENY: Policy = Policy {
    name: "custom-deny",
    verify: custom_deny_verify,
};

fn custom_deny_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    let patterns = CUSTOM_DENY_PATTERNS.get()?;
    for arg in &seg.argv {
        if token_matches_deny(arg, patterns) {
            return Some(Verdict::Deny);
        }
    }
    for redir in &seg.redirects {
        if token_matches_deny(&redir.target, patterns) {
            return Some(Verdict::Deny);
        }
    }
    None
}

fn token_matches_deny(token: &str, patterns: &[String]) -> bool {
    let p = Path::new(token);
    for pattern in patterns {
        let (prefix, is_glob) = match pattern.strip_suffix('*') {
            Some(p) => (p, true),
            None => (pattern.as_str(), false),
        };
        for component in p.components() {
            if let std::path::Component::Normal(c) = component {
                let s = match c.to_str() {
                    Some(s) => s,
                    None => continue,
                };
                if is_glob {
                    if s.starts_with(prefix) {
                        return true;
                    }
                } else if s == prefix {
                    return true;
                }
            }
        }
    }
    false
}

fn custom_allow_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    let rules = CUSTOM_ALLOW_RULES.get()?;
    if seg.argv.is_empty() {
        return None;
    }
    let bin = argv0_basename(&seg.argv[0]);
    for rule in rules {
        if rule.len() > seg.argv.len() {
            continue;
        }
        // First token matches by basename (so `/usr/bin/just` matches `just`)
        // or by literal argv[0]; later tokens must match exactly.
        let head_ok = bin == rule[0].as_str() || seg.argv[0] == rule[0];
        if !head_ok {
            continue;
        }
        if rule.iter().skip(1).zip(seg.argv.iter().skip(1)).all(|(r, a)| r == a) {
            return Some(Verdict::Approve);
        }
    }
    None
}

fn argv0_basename(s: &str) -> &str {
    Path::new(s).file_name().and_then(|n| n.to_str()).unwrap_or(s)
}

fn add_capabilities(
    policies: &mut Vec<&'static Policy>,
    tool: &str,
    list: &str,
) -> Result<(), String> {
    for cap in list.split(',') {
        let cap = cap.trim();
        if cap.is_empty() {
            continue;
        }
        let full = format!("{tool}:{cap}");
        let p = policies::lookup(&full)
            .ok_or_else(|| format!("unknown {tool} capability: {cap}"))?;
        policies.push(p);
    }
    Ok(())
}

fn print_usage() {
    eprintln!(
        "da {} — yes.\n\n\
        usage:\n  \
        da --path PATH [policy flags...]\n\n\
        stdin: command string to classify\n\
        exit:  0 = approve, 1 = defer, 2 = deny, 64 = usage error\n\n\
        policy flags (pass at least one):\n  \
        --read-only             cross-platform read-only binaries\n  \
        --macos-only            macOS-specific read-only extras\n  \
        --help-bypass           <bin> --help|-h|--version|-V|help|version (any binary)\n  \
        --mkdir-cwd             mkdir bounded to --path\n  \
        --git    CAPS           git capabilities (csv): read,add,commit,\n  \
                                restore-staged,tag,fetch,pull,push\n  \
        --cargo  CAPS           cargo capabilities (csv): local,\n  \
                                crates-install,crates-publish\n  \
        --allow  CMD            token-prefix rule (repeatable). Approves any segment\n  \
                                whose argv starts with CMD's tokens. Path components\n  \
                                of argv[0] are stripped before matching.\n  \
        --deny   PATTERN        path-component deny rule (repeatable). Denies any\n  \
                                segment whose argv or redirect targets contain a\n  \
                                matching path component. Trailing * = prefix match.\n  \
                                Deny rules override allow rules.\n\n\
        examples:\n  \
        printf '%s' 'ls -la'         | da --read-only\n  \
        printf '%s' 'git status'     | da --git read\n  \
        printf '%s' 'git add foo.rs' | da --git read,add\n  \
        printf '%s' 'mkdir foo'      | da --path /repo --mkdir-cwd\n  \
        printf '%s' 'just test'      | da --allow 'just test'\n  \
        printf '%s' 'npm run build'  | da --allow 'npm run'\n  \
        printf '%s' 'cat .env'       | da --read-only --deny '.env*'\n  \
        printf '%s' 'cat ~/.ssh/key' | da --read-only --deny .ssh\n",
        env!("CARGO_PKG_VERSION"),
    );
}
