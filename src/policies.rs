//! Built-in policies. Each capability is one [`Policy`] value with one
//! verify fn. Atomic: nothing subsumes anything else; users compose by
//! listing the capabilities they want.
//!
//! External callers can define their own [`Policy`] values and mix them
//! with the built-ins.
//!
//! [`Policy`]: crate::Policy

use std::path::Path;

use crate::engine::argv0_name;
use crate::shparse::Segment;
use crate::{Policy, Verdict};

/// Convenience slice listing every built-in policy. Useful for the
/// kitchen-sink case in tests, or as a starting point for trimming.
pub const ALL_BUILTIN: &[&Policy] = &[
    &READ_ONLY, &MACOS_ONLY, &HELP_BYPASS, &MKDIR_CWD,
    &GIT_READ, &GIT_ADD, &GIT_COMMIT, &GIT_RESTORE_STAGED,
    &GIT_TAG, &GIT_FETCH, &GIT_PULL, &GIT_PUSH,
    &CARGO_LOCAL, &CARGO_CRATES_INSTALL, &CARGO_CRATES_PUBLISH,
];

// ─── read-only ──────────────────────────────────────────────────────────────

/// Cross-platform read-only binaries plus bounded forms of `sed`/`awk`/
/// `find`/`sysctl`. macOS-specific extras live under [`MACOS_ONLY`].
pub static READ_ONLY: Policy = Policy {
    name: "read-only",
    verify: read_only_verify,
};

const READ_ONLY_BINS: &[&str] = &[
    // File discovery & listing
    "find", "ls", "tree", "exa", "eza", "fd", "locate",
    // File info & checksums
    "stat", "file", "wc", "du", "df",
    "md5", "md5sum", "shasum", "sha256sum", "sha1sum", "sha512sum",
    "b2sum", "cksum", "sum", "base64",
    // File reading
    "cat", "head", "tail", "less", "more", "bat",
    // Compressed file viewing
    "zcat", "bzcat", "xzcat", "zstdcat",
    "zless", "zmore", "xzless", "xzmore", "bzmore",
    "zipinfo",
    // Search
    "grep", "egrep", "fgrep", "rg", "ag", "ack",
    // Text processing
    "sort", "uniq", "cut", "tr", "rev", "tac", "nl",
    "column", "paste", "fold", "expand", "unexpand",
    "fmt", "join", "col", "colrm", "tsort",
    "awk", "gawk", "mawk", "nawk",
    "sed",
    // Comparison
    "diff", "diff3", "comm", "cmp",
    // JSON / structured data
    "jq", "yq", "xq", "xmllint",
    // Path utilities
    "dirname", "basename", "realpath", "readlink", "pwd",
    // System info
    "date", "cal", "uname", "whoami", "hostname", "id", "uptime",
    "arch", "nproc", "getconf",
    "sysctl",
    "locale", "groups", "who", "w", "last", "logname", "users",
    // Process inspection
    "ps", "pgrep", "top", "htop", "btop", "lsof",
    "vmstat", "iostat", "free",
    // Network diagnostics
    "dig", "nslookup", "host", "ping",
    "traceroute", "tracepath", "mtr",
    "ss", "netstat",
    // Binary inspection
    "ldd", "nm", "objdump", "readelf", "size",
    "dwarfdump", "strings",
    // Tool lookup
    "which", "whereis", "where", "type", "command", "hash",
    "man", "apropos", "whatis", "info", "tldr", "help",
    // Output / control
    "echo", "printf", "true", "false", "test", "[",
    // Environment
    "printenv",
    // Media / document metadata
    "ffprobe", "mediainfo", "pdfinfo", "identify", "soxi",
    // Terminal
    "clear", "tput", "tty", "reset",
    // Misc
    "seq", "expr", "bc", "dc", "xxd", "od",
    "hexdump", "sleep", "cloc",
];

const FIND_DANGEROUS: &[&str] = &[
    "-exec", "-execdir", "-delete", "-ok", "-okdir",
    "-fprint", "-fprint0", "-fprintf",
];

const SED_DANGEROUS_FLAGS: &[&str] = &["-i", "--in-place"];

fn read_only_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    if seg.argv.is_empty() {
        return None;
    }
    let bin = argv0_name(&seg.argv[0]);
    if !READ_ONLY_BINS.contains(&bin) {
        return None;
    }
    let ok = match bin {
        "find" => find_safe(&seg.argv),
        "sed" => sed_safe(&seg.argv),
        "awk" | "gawk" | "mawk" | "nawk" => awk_safe(&seg.argv),
        "sysctl" => sysctl_safe(&seg.argv),
        _ => true,
    };
    if ok {
        Some(Verdict::Approve)
    } else {
        None
    }
}

fn find_safe(argv: &[String]) -> bool {
    !argv.iter().any(|t| FIND_DANGEROUS.contains(&t.as_str()))
}

fn sed_safe(argv: &[String]) -> bool {
    if argv.iter().any(|t| {
        SED_DANGEROUS_FLAGS.contains(&t.as_str()) || t.starts_with("-i")
    }) {
        return false;
    }
    !argv.iter().any(|t| sed_script_has_exec_flag(t))
}

fn sed_script_has_exec_flag(t: &str) -> bool {
    if t.len() < 4 {
        return false;
    }
    let bytes = t.as_bytes();
    if bytes[0] != b's' || bytes[1].is_ascii_alphanumeric() {
        return false;
    }
    let delim = bytes[1];
    let delim_count = bytes[1..].iter().filter(|&&b| b == delim).count();
    if delim_count < 3 {
        return false;
    }
    let mut count = 0;
    let mut flags_start = 0;
    for (j, &b) in bytes[1..].iter().enumerate() {
        if b == delim {
            count += 1;
            if count == 3 {
                flags_start = j + 2;
                break;
            }
        }
    }
    flags_start > 0 && flags_start < t.len() && t[flags_start..].contains('e')
}

fn awk_safe(argv: &[String]) -> bool {
    argv.iter().all(|t| {
        if t.contains("system(") {
            return false;
        }
        for pat in ["> \"", ">> \"", "| \"", ">\"", ">>\"", "|\""] {
            if t.contains(pat) {
                return false;
            }
        }
        true
    })
}

fn sysctl_safe(argv: &[String]) -> bool {
    !argv.iter().any(|t| t == "-w" || t.starts_with("-w"))
}

// ─── macos-only ─────────────────────────────────────────────────────────────

/// macOS-specific read-only extras. No-op on non-macOS hosts (the binaries
/// just don't exist) — but kept opt-in to stay explicit.
pub static MACOS_ONLY: Policy = Policy {
    name: "macos-only",
    verify: macos_only_verify,
};

const MACOS_ONLY_BINS: &[&str] = &[
    "mdfind", "mdls",
    "sw_vers", "system_profiler", "hostinfo", "vm_stat",
    "pbpaste",
    "otool", "dyld_info",
];

fn macos_only_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    if seg.argv.is_empty() {
        return None;
    }
    let bin = argv0_name(&seg.argv[0]);
    if MACOS_ONLY_BINS.contains(&bin) {
        Some(Verdict::Approve)
    } else {
        None
    }
}

// ─── help-bypass ────────────────────────────────────────────────────────────

/// `<binary> --help|-h|--version|-V|help|version` for *any* binary, even
/// ones that aren't otherwise approved. Lets unknown binaries run, even if
/// just for help — explicit trust call.
pub static HELP_BYPASS: Policy = Policy {
    name: "help-bypass",
    verify: help_bypass_verify,
};

fn help_bypass_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    if seg.argv.len() == 2 && is_help_flag(&seg.argv[1]) {
        Some(Verdict::Approve)
    } else {
        None
    }
}

fn is_help_flag(s: &str) -> bool {
    matches!(s, "--help" | "-h" | "--version" | "-V" | "help" | "version")
}

// ─── mkdir-cwd ──────────────────────────────────────────────────────────────

/// `mkdir` when every path argument resolves under the `--path` directory.
/// Requires the engine caller to pass a path (otherwise only relative paths
/// without `..` traversal are accepted).
pub static MKDIR_CWD: Policy = Policy {
    name: "mkdir-cwd",
    verify: mkdir_cwd_verify,
};

fn mkdir_cwd_verify(seg: &Segment, path: Option<&Path>) -> Option<Verdict> {
    if seg.argv.is_empty() || argv0_name(&seg.argv[0]) != "mkdir" {
        return None;
    }
    let cwd_str = path.and_then(|p| p.to_str());
    let mut i = 1;
    let mut has_path = false;
    while i < seg.argv.len() {
        let t = &seg.argv[i];
        if t == "-m" || t == "--mode" {
            i += 2;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        if !is_subdir_of_cwd(t, cwd_str) {
            return None;
        }
        has_path = true;
        i += 1;
    }
    if has_path {
        Some(Verdict::Approve)
    } else {
        None
    }
}

fn is_subdir_of_cwd(path: &str, cwd: Option<&str>) -> bool {
    // argv strings are quote-decoded; literal $ or leading ~ here means the
    // user wrote them quoted (so they're meant literally) — reject as odd.
    if path.is_empty() || path.contains('$') || path.starts_with('~') {
        return false;
    }
    let cwd_str = match cwd {
        Some(c) if c.starts_with('/') => c,
        _ => {
            return !path.starts_with('/') && !path.split('/').any(|s| s == "..");
        }
    };
    let mut stack: Vec<&str> = if path.starts_with('/') {
        Vec::new()
    } else {
        cwd_str.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in path.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            if stack.pop().is_none() {
                return false;
            }
            continue;
        }
        stack.push(seg);
    }
    let cwd_stack: Vec<&str> = cwd_str.split('/').filter(|s| !s.is_empty()).collect();
    if stack.len() < cwd_stack.len() {
        return false;
    }
    cwd_stack.iter().zip(stack.iter()).all(|(a, b)| a == b)
}

// ─── git:* family ───────────────────────────────────────────────────────────

const GIT_READ_SUBCOMMANDS: &[&str] = &[
    "status", "log", "diff", "show", "branch", "blame",
    "ls-files", "ls-tree", "rev-parse", "rev-list", "describe",
    "reflog", "shortlog", "config",
];
const GIT_BRANCH_DANGEROUS: &[&str] =
    &["-D", "-d", "--delete", "-M", "-m", "--move", "--force", "-f"];
const GIT_CONFIG_DANGEROUS: &[&str] = &[
    "--global", "--system", "--add", "--unset", "--unset-all",
    "--replace-all", "--rename-section", "--remove-section",
];
const GIT_COMMIT_DANGEROUS: &[&str] = &["--amend", "--no-verify", "--no-gpg-sign"];

/// git inspection: status, log, diff, show, branch (read), blame, ls-files,
/// ls-tree, rev-parse, rev-list, describe, reflog, shortlog, config --get.
/// Branch with `-d/-D/-m/--force` and config with mutating flags are
/// rejected.
pub static GIT_READ: Policy = Policy {
    name: "git:read",
    verify: git_read_verify,
};

fn git_read_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    let sub = git_subcommand(seg)?;
    if !GIT_READ_SUBCOMMANDS.contains(&sub) {
        return None;
    }
    if sub == "branch" && seg.argv.iter().any(|t| GIT_BRANCH_DANGEROUS.contains(&t.as_str())) {
        return None;
    }
    if sub == "config" && seg.argv.iter().any(|t| GIT_CONFIG_DANGEROUS.contains(&t.as_str())) {
        return None;
    }
    Some(Verdict::Approve)
}

/// `git add` — staging.
pub static GIT_ADD: Policy = Policy {
    name: "git:add",
    verify: git_match_add,
};

/// `git commit` — excludes `--amend`, `--no-verify`, `--no-gpg-sign`.
pub static GIT_COMMIT: Policy = Policy {
    name: "git:commit",
    verify: git_commit_verify,
};

fn git_commit_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    let sub = git_subcommand(seg)?;
    if sub != "commit" {
        return None;
    }
    if seg.argv.iter().any(|t| GIT_COMMIT_DANGEROUS.contains(&t.as_str())) {
        return None;
    }
    Some(Verdict::Approve)
}

/// `git restore --staged` (or `-S`) — unstage. `git restore` against the
/// working tree is destructive and isn't covered by any built-in.
pub static GIT_RESTORE_STAGED: Policy = Policy {
    name: "git:restore-staged",
    verify: git_restore_staged_verify,
};

fn git_restore_staged_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    let sub = git_subcommand(seg)?;
    if sub != "restore" {
        return None;
    }
    if seg.argv.iter().any(|t| t == "--staged" || t == "-S") {
        Some(Verdict::Approve)
    } else {
        None
    }
}

/// `git tag` — create/list/delete tags. Local until pushed.
pub static GIT_TAG: Policy = Policy {
    name: "git:tag",
    verify: git_match_tag,
};

/// `git push` — network write.
pub static GIT_PUSH: Policy = Policy {
    name: "git:push",
    verify: git_match_push,
};

/// `git pull` — network sync (fetch + merge).
pub static GIT_PULL: Policy = Policy {
    name: "git:pull",
    verify: git_match_pull,
};

/// `git fetch` — network sync (fetch only).
pub static GIT_FETCH: Policy = Policy {
    name: "git:fetch",
    verify: git_match_fetch,
};

fn git_match_add(seg: &Segment, _p: Option<&Path>) -> Option<Verdict> {
    if git_subcommand(seg)? == "add" { Some(Verdict::Approve) } else { None }
}
fn git_match_tag(seg: &Segment, _p: Option<&Path>) -> Option<Verdict> {
    if git_subcommand(seg)? == "tag" { Some(Verdict::Approve) } else { None }
}
fn git_match_push(seg: &Segment, _p: Option<&Path>) -> Option<Verdict> {
    if git_subcommand(seg)? == "push" { Some(Verdict::Approve) } else { None }
}
fn git_match_pull(seg: &Segment, _p: Option<&Path>) -> Option<Verdict> {
    if git_subcommand(seg)? == "pull" { Some(Verdict::Approve) } else { None }
}
fn git_match_fetch(seg: &Segment, _p: Option<&Path>) -> Option<Verdict> {
    if git_subcommand(seg)? == "fetch" { Some(Verdict::Approve) } else { None }
}

fn git_subcommand(seg: &Segment) -> Option<&str> {
    if seg.argv.is_empty() || argv0_name(&seg.argv[0]) != "git" {
        return None;
    }
    let mut i = 1;
    while i < seg.argv.len() {
        let t = &seg.argv[i];
        if t == "-C" || t == "-c" {
            i += 2;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(t.as_str());
    }
    None
}

// ─── cargo:* family ─────────────────────────────────────────────────────────

const CARGO_LOCAL_SUBCOMMANDS: &[&str] = &[
    "check", "build", "b", "c",
    "doc", "d", "rustdoc",
    "tree", "metadata", "search", "read-manifest", "locate-project",
    "pkgid", "verify-project", "fetch", "help", "version",
    "test", "t", "bench", "update",
    "run", "r",
    "fmt", "clippy", "clean",
];

/// All cargo operations that don't reach a registry mutably. Bundles
/// build/test/run/fmt/clippy/check/doc/etc. — local actions only. Does
/// **not** subsume `crates-install` or `crates-publish`.
pub static CARGO_LOCAL: Policy = Policy {
    name: "cargo:local",
    verify: cargo_local_verify,
};

fn cargo_local_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    let sub = cargo_subcommand(seg)?;
    if !CARGO_LOCAL_SUBCOMMANDS.contains(&sub) {
        return None;
    }
    if seg.argv.iter().any(|t| t == "--fix") {
        return None;
    }
    Some(Verdict::Approve)
}

/// `cargo install` — downloads, compiles, installs from crates.io.
pub static CARGO_CRATES_INSTALL: Policy = Policy {
    name: "cargo:crates-install",
    verify: cargo_crates_install_verify,
};

fn cargo_crates_install_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    if cargo_subcommand(seg)? == "install" { Some(Verdict::Approve) } else { None }
}

/// `cargo publish` — uploads to crates.io.
pub static CARGO_CRATES_PUBLISH: Policy = Policy {
    name: "cargo:crates-publish",
    verify: cargo_crates_publish_verify,
};

fn cargo_crates_publish_verify(seg: &Segment, _path: Option<&Path>) -> Option<Verdict> {
    if cargo_subcommand(seg)? == "publish" { Some(Verdict::Approve) } else { None }
}

fn cargo_subcommand(seg: &Segment) -> Option<&str> {
    if seg.argv.is_empty() || argv0_name(&seg.argv[0]) != "cargo" {
        return None;
    }
    let mut i = 1;
    while i < seg.argv.len() {
        let t = &seg.argv[i];
        if t.starts_with('+') {
            i += 1;
            continue;
        }
        if matches!(t.as_str(), "--color" | "--config" | "-Z" | "-C") {
            i += 2;
            continue;
        }
        if t.starts_with('-') {
            i += 1;
            continue;
        }
        return Some(t.as_str());
    }
    None
}

// ─── lookup helper for the CLI ──────────────────────────────────────────────

/// Map a `tool:capability` name (or a flat name like `read-only`) to its
/// built-in policy. Returns `None` for unknown names. Used by the CLI to
/// translate `--git read,add` into a policy stack.
pub fn lookup(name: &str) -> Option<&'static Policy> {
    Some(match name {
        "read-only"             => &READ_ONLY,
        "macos-only"            => &MACOS_ONLY,
        "help-bypass"           => &HELP_BYPASS,
        "mkdir-cwd"             => &MKDIR_CWD,
        "git:read"              => &GIT_READ,
        "git:add"               => &GIT_ADD,
        "git:commit"            => &GIT_COMMIT,
        "git:restore-staged"    => &GIT_RESTORE_STAGED,
        "git:tag"               => &GIT_TAG,
        "git:fetch"             => &GIT_FETCH,
        "git:pull"              => &GIT_PULL,
        "git:push"              => &GIT_PUSH,
        "cargo:local"           => &CARGO_LOCAL,
        "cargo:crates-install"  => &CARGO_CRATES_INSTALL,
        "cargo:crates-publish"  => &CARGO_CRATES_PUBLISH,
        _ => return None,
    })
}

// ─── tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{classify, Decision};

    fn approves(cmd: &str, pol: &[&Policy]) -> bool {
        matches!(classify(cmd, None, pol), Decision::Approve)
    }
    fn defers(cmd: &str, pol: &[&Policy]) -> bool {
        matches!(classify(cmd, None, pol), Decision::Defer)
    }

    // ── read-only ────────────────────────────────────────────────────────

    #[test]
    fn read_only_basics() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(approves("ls -la", p));
        assert!(approves("grep foo bar.txt", p));
        assert!(approves("cat README.md", p));
        assert!(approves("jq '.name' package.json", p));
        assert!(approves("ps aux", p));
        assert!(approves("dig example.com", p));
        assert!(approves("ping -c 1 example.com", p));
    }

    #[test]
    fn read_only_rejects_unknown() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(defers("rm -rf /", p));
        assert!(defers("curl https://example.com", p));
        assert!(defers("python script.py", p));
    }

    #[test]
    fn read_only_sed_in_place_blocked() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(defers("sed -i 's/a/b/' f.txt", p));
        assert!(defers("sed -i.bak 's/a/b/' f.txt", p));
        assert!(defers("sed -i'' 's/a/b/' f.txt", p));
        assert!(defers("sed --in-place 's/a/b/' f.txt", p));
    }

    #[test]
    fn read_only_sed_exec_flag_blocked() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(defers("sed 's/a/b/e' f.txt", p));
        assert!(defers("sed 's/a/b/ge' f.txt", p));
        assert!(defers("sed 's|a|b|e' f.txt", p));
    }

    #[test]
    fn read_only_sed_safe() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(approves("sed 's/a/b/' f.txt", p));
        assert!(approves("sed 's/a/b/g' f.txt", p));
        assert!(approves("sed -n 's/a/b/p' f.txt", p));
    }

    #[test]
    fn read_only_awk_system_blocked() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(defers("awk 'BEGIN{system(\"rm -rf /\")}'", p));
        assert!(defers("awk '{system(\"id\")}'", p));
    }

    #[test]
    fn read_only_awk_file_write_blocked() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(defers("awk '{print > \"out.txt\"}'", p));
        assert!(defers("awk '{print >> \"out.txt\"}'", p));
        assert!(defers("awk '{print | \"mail u@example.com\"}'", p));
    }

    #[test]
    fn read_only_awk_safe() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(approves("awk '{print $2}' f.txt", p));
        assert!(approves("awk -F: '{print $1}' /etc/passwd", p));
    }

    #[test]
    fn read_only_sysctl_read_safe_write_blocked() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(approves("sysctl kern.ostype", p));
        assert!(approves("sysctl -a", p));
        assert!(defers("sysctl -w net.ipv4.ip_forward=1", p));
    }

    #[test]
    fn read_only_find_safe() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(approves("find . -name '*.rs'", p));
        assert!(approves("find . -name '*.rs' -type f", p));
    }

    #[test]
    fn read_only_find_dangerous_blocked() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(defers("find . -name '*.rs' -exec rm {} \\;", p));
        assert!(defers("find . -delete", p));
        assert!(defers("find . -execdir rm {} \\;", p));
        assert!(defers("find . -fprint results.txt", p));
    }

    #[test]
    fn read_only_pipeline() {
        let p: &[&Policy] = &[&READ_ONLY];
        assert!(approves("find . -name '*.rs' -type f | sort | uniq | wc -l", p));
        assert!(approves("LC_ALL=C sort f.txt | uniq -c | head -20", p));
    }

    // ── macos-only ───────────────────────────────────────────────────────

    #[test]
    fn macos_only() {
        let p: &[&Policy] = &[&MACOS_ONLY];
        assert!(approves("pbpaste", p));
        assert!(approves("system_profiler SPHardwareDataType", p));
        assert!(approves("sw_vers", p));
        assert!(approves("mdfind kMDItemContentType=public.image", p));
        assert!(approves("otool -L binary", p));
        assert!(defers("ls", p));
    }

    // ── help-bypass ──────────────────────────────────────────────────────

    #[test]
    fn help_bypass_unknown_binary() {
        let p: &[&Policy] = &[&HELP_BYPASS];
        assert!(approves("terraform --help", p));
        assert!(approves("kubectl -h", p));
        assert!(approves("docker --version", p));
        assert!(approves("npm --version", p));
        assert!(approves("kubectl version", p));
        assert!(approves("cargo help", p));
    }

    #[test]
    fn help_bypass_requires_two_args() {
        let p: &[&Policy] = &[&HELP_BYPASS];
        assert!(defers("terraform plan --help", p));
        assert!(defers("docker", p));
    }

    // ── mkdir-cwd ────────────────────────────────────────────────────────

    fn approves_with_path(cmd: &str, path: &str, pol: &[&Policy]) -> bool {
        let p = Path::new(path);
        matches!(classify(cmd, Some(p), pol), Decision::Approve)
    }
    fn defers_with_path(cmd: &str, path: &str, pol: &[&Policy]) -> bool {
        let p = Path::new(path);
        matches!(classify(cmd, Some(p), pol), Decision::Defer)
    }

    #[test]
    fn mkdir_relative_subdir_safe() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(approves_with_path("mkdir foo", cwd, p));
        assert!(approves_with_path("mkdir -p foo/bar", cwd, p));
        assert!(approves_with_path("mkdir -pv src/components", cwd, p));
        assert!(approves_with_path("mkdir --parents deeply/nested/path", cwd, p));
        assert!(approves_with_path("mkdir a b c", cwd, p));
        assert!(approves_with_path("mkdir ./foo", cwd, p));
    }

    #[test]
    fn mkdir_absolute_under_cwd_safe() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(approves_with_path("mkdir /home/user/project/foo", cwd, p));
        assert!(approves_with_path("mkdir -p /home/user/project/src/components", cwd, p));
    }

    #[test]
    fn mkdir_absolute_outside_cwd_blocked() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(defers_with_path("mkdir /tmp/foo", cwd, p));
        assert!(defers_with_path("mkdir -p /etc/thing", cwd, p));
        assert!(defers_with_path("mkdir -p /home/user/other/foo", cwd, p));
        assert!(defers_with_path("mkdir -p /home/user", cwd, p));
    }

    #[test]
    fn mkdir_parent_traversal_blocked() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(defers_with_path("mkdir ../foo", cwd, p));
        assert!(defers_with_path("mkdir -p foo/../../bar", cwd, p));
    }

    #[test]
    fn mkdir_traversal_back_into_cwd_safe() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(approves_with_path("mkdir -p foo/../bar", cwd, p));
    }

    #[test]
    fn mkdir_home_and_var_blocked() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(defers_with_path("mkdir ~/foo", cwd, p));
        assert!(defers_with_path("mkdir $HOME/foo", cwd, p));
    }

    #[test]
    fn mkdir_mode_flag_consumes_arg() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(approves_with_path("mkdir -m 755 foo", cwd, p));
        assert!(approves_with_path("mkdir --mode 0750 foo", cwd, p));
        assert!(approves_with_path("mkdir -pm 755 foo/bar", cwd, p));
    }

    #[test]
    fn mkdir_no_paths_defers() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(defers_with_path("mkdir", cwd, p));
        assert!(defers_with_path("mkdir -p", cwd, p));
    }

    #[test]
    fn mkdir_quoted_path() {
        let p: &[&Policy] = &[&MKDIR_CWD];
        let cwd = "/home/user/project";
        assert!(approves_with_path("mkdir -p \"/home/user/project/foo\"", cwd, p));
        assert!(approves_with_path("mkdir -p '/home/user/project/foo'", cwd, p));
        assert!(defers_with_path("mkdir -p '/tmp/foo'", cwd, p));
    }

    // ── git family ───────────────────────────────────────────────────────

    #[test]
    fn git_read_safe() {
        let p: &[&Policy] = &[&GIT_READ];
        assert!(approves("git status", p));
        assert!(approves("git log --oneline -10", p));
        assert!(approves("git diff --stat", p));
        assert!(approves("git show HEAD", p));
        assert!(approves("git branch", p));
        assert!(approves("git blame foo.rs", p));
        assert!(approves("git rev-parse HEAD", p));
    }

    #[test]
    fn git_global_flags_skipped() {
        let p: &[&Policy] = &[&GIT_READ];
        assert!(approves("git -C /tmp status", p));
        assert!(approves("git -c color.ui=always log", p));
    }

    #[test]
    fn git_branch_dangerous_flags_blocked_under_read() {
        let p: &[&Policy] = &[&GIT_READ];
        assert!(defers("git branch -D feature", p));
        assert!(defers("git branch -d feature", p));
        assert!(defers("git branch -m old new", p));
        assert!(defers("git branch --force feature main", p));
    }

    #[test]
    fn git_config_get_safe_write_blocked() {
        let p: &[&Policy] = &[&GIT_READ];
        assert!(approves("git config --get user.name", p));
        assert!(defers("git config --global user.name x", p));
        assert!(defers("git config --unset user.name", p));
    }

    #[test]
    fn git_add_only_with_add() {
        let p: &[&Policy] = &[&GIT_ADD];
        assert!(approves("git add Cargo.toml", p));
        assert!(approves("git add .", p));
        assert!(defers("git status", p));
    }

    #[test]
    fn git_commit_safe_vs_dangerous() {
        let p: &[&Policy] = &[&GIT_COMMIT];
        assert!(approves("git commit -m wip", p));
        assert!(defers("git commit --amend", p));
        assert!(defers("git commit --no-verify -m x", p));
        assert!(defers("git commit --no-gpg-sign -m x", p));
    }

    #[test]
    fn git_restore_staged_only_with_flag() {
        let p: &[&Policy] = &[&GIT_RESTORE_STAGED];
        assert!(approves("git restore --staged foo.txt", p));
        assert!(approves("git restore -S foo.txt", p));
        assert!(defers("git restore foo.txt", p));
    }

    #[test]
    fn git_tag_approves() {
        let p: &[&Policy] = &[&GIT_TAG];
        assert!(approves("git tag v1.0", p));
        assert!(approves("git tag -l", p));
        assert!(approves("git tag -d old", p));
    }

    #[test]
    fn git_network_each_separate() {
        assert!(approves("git fetch", &[&GIT_FETCH]));
        assert!(approves("git pull", &[&GIT_PULL]));
        assert!(approves("git push", &[&GIT_PUSH]));
        // No subsumption: git-fetch alone doesn't approve push.
        assert!(defers("git push", &[&GIT_FETCH]));
    }

    #[test]
    fn git_destructive_never_approves() {
        let p: &[&Policy] = &[
            &GIT_READ, &GIT_ADD, &GIT_COMMIT, &GIT_RESTORE_STAGED,
            &GIT_TAG, &GIT_FETCH, &GIT_PULL, &GIT_PUSH,
        ];
        assert!(defers("git reset --hard HEAD", p));
        assert!(defers("git rebase main", p));
        assert!(defers("git merge feature", p));
        assert!(defers("git checkout .", p));
        assert!(defers("git clean -fd", p));
        assert!(defers("git rm foo.txt", p));
        assert!(defers("git stash", p));
        assert!(defers("git cherry-pick abc", p));
    }

    // ── cd + git compounds ───────────────────────────────────────────────

    #[test]
    fn cd_then_git_compound() {
        let p: &[&Policy] = &[&GIT_READ];
        assert!(approves("cd /tmp && git status", p));
        let p2: &[&Policy] = &[&GIT_READ, &GIT_ADD, &GIT_COMMIT];
        assert!(approves("cd /tmp && git add . && git commit -m wip", p2));
        // push not enabled → defer
        assert!(defers("cd /tmp && git push", p2));
    }

    // ── cargo family ─────────────────────────────────────────────────────

    #[test]
    fn cargo_local_safe() {
        let p: &[&Policy] = &[&CARGO_LOCAL];
        assert!(approves("cargo check", p));
        assert!(approves("cargo build --release", p));
        assert!(approves("cargo test", p));
        assert!(approves("cargo doc -p tracing-subscriber --no-deps", p));
        assert!(approves("cargo tree -i serde", p));
        assert!(approves("cargo metadata --format-version 1", p));
        assert!(approves("cargo run", p));
        assert!(approves("cargo fmt", p));
        assert!(approves("cargo clippy", p));
        assert!(approves("cargo update", p));
        assert!(approves("cargo update -p some-dep", p));
        assert!(approves(
            "cargo update --manifest-path tools/x/Cargo.toml -p some-dep",
            p,
        ));
        assert!(approves("cargo clean", p));
    }

    #[test]
    fn cargo_local_does_not_subsume_install_publish() {
        let p: &[&Policy] = &[&CARGO_LOCAL];
        assert!(defers("cargo install some-dep", p));
        assert!(defers("cargo publish", p));
    }

    #[test]
    fn cargo_install_explicit() {
        let p: &[&Policy] = &[&CARGO_CRATES_INSTALL];
        assert!(approves("cargo install dabin", p));
        assert!(defers("cargo build", p));
    }

    #[test]
    fn cargo_publish_explicit() {
        let p: &[&Policy] = &[&CARGO_CRATES_PUBLISH];
        assert!(approves("cargo publish", p));
        assert!(defers("cargo install dabin", p));
    }

    #[test]
    fn cargo_toolchain_override_skipped() {
        let p: &[&Policy] = &[&CARGO_LOCAL];
        assert!(approves("cargo +nightly check", p));
    }

    #[test]
    fn cargo_fix_flag_blocked() {
        let p: &[&Policy] = &[&CARGO_LOCAL];
        assert!(defers("cargo check --fix", p));
    }

    #[test]
    fn cargo_unknown_subcommand_defers() {
        let p: &[&Policy] = &[&CARGO_LOCAL];
        assert!(defers("cargo new myproj", p));
    }

    // ── help-bypass + cargo interplay ────────────────────────────────────

    #[test]
    fn help_bypass_works_alongside_cargo() {
        let p: &[&Policy] = &[&HELP_BYPASS, &CARGO_LOCAL];
        assert!(approves("cargo --help", p));
        assert!(approves("cargo build --help", p));
        assert!(approves("terraform --help", p));
    }

    // ── lookup ───────────────────────────────────────────────────────────

    #[test]
    fn lookup_resolves_built_ins() {
        assert!(lookup("read-only").is_some());
        assert!(lookup("git:read").is_some());
        assert!(lookup("cargo:local").is_some());
        assert!(lookup("nonsense").is_none());
    }
}
