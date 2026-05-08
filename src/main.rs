//! `da` — read a bash command on stdin, classify it under the policies
//! enabled by flags, exit 0/1/2/64 (approve/defer/deny/usage error).
//!
//! No JSON, no defaults, no implicit anything. Wrappers (e.g. for Claude
//! Code's hook protocol) live with the caller.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;

use dabin::policies::{self, HELP_BYPASS, MACOS_ONLY, MKDIR_CWD, READ_ONLY};
use dabin::{classify, Decision, Policy};

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

    let decision = classify(cmd, parsed.path.as_deref(), &parsed.policies);
    Ok(match decision {
        Decision::Approve => EXIT_APPROVE,
        Decision::Defer => EXIT_DEFER,
        Decision::Deny => EXIT_DENY,
    })
}

struct Parsed {
    path: Option<PathBuf>,
    policies: Vec<&'static Policy>,
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
            other => return Err(format!("unknown flag: {other}")),
        }
    }

    if policies.is_empty() {
        return Err("no policies enabled — pass at least one policy flag".into());
    }
    Ok(Parsed { path, policies })
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
                                crates-install,crates-publish\n\n\
        examples:\n  \
        printf '%s' 'ls -la'         | da --read-only\n  \
        printf '%s' 'git status'     | da --git read\n  \
        printf '%s' 'git add foo.rs' | da --git read,add\n  \
        printf '%s' 'mkdir foo'      | da --path /repo --mkdir-cwd\n",
        env!("CARGO_PKG_VERSION"),
    );
}
