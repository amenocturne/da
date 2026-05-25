#!/usr/bin/env python3
"""Rule-based bash command safety labeler."""

import json
import re
import sys
from pathlib import Path

SAFE_FIRST_TOKENS = {
    "ls", "eza", "exa", "tree", "find", "locate",
    "cat", "head", "tail", "less", "more", "bat",
    "grep", "rg", "ag", "ack", "ripgrep",
    "wc", "file", "stat", "du", "df",
    "echo", "printf",
    "uname", "sw_vers", "arch", "hostname", "whoami", "id",
    "ps", "top", "htop", "btop",
    "which", "type", "command", "where", "whence",
    "date", "cal",
    "jq", "yq", "xq",
    "ffprobe", "exiftool", "mediainfo", "identify",
    "diff", "colordiff", "delta",
    "env", "printenv", "set",
    "pwd", "realpath", "dirname", "basename",
    "true", "false", "test", "[",
    "seq", "sort", "uniq", "tr", "cut", "paste", "column", "rev",
    "md5", "md5sum", "sha256sum", "shasum",
    "strings", "xxd", "hexdump", "od",
    "tput", "tty",
    "nproc", "getconf",
    "man", "help", "info",
}

SAFE_GIT_SUBCOMMANDS = {
    "status", "log", "diff", "show", "branch", "tag",
    "remote", "stash", "ls-files", "ls-tree", "rev-parse",
    "describe", "shortlog", "blame", "reflog", "config",
    "rev-list", "cat-file", "for-each-ref", "name-rev",
    "symbolic-ref", "merge-base", "count-objects",
}

NEEDS_APPROVAL_GIT_SUBCOMMANDS = {
    "add", "commit", "pull", "fetch", "merge", "rebase",
    "cherry-pick", "switch", "restore", "worktree",
    "submodule", "init", "clone", "am", "apply",
    "bisect", "clean", "gc", "prune", "notes",
}

DANGEROUS_PATTERNS = [
    (r"\bcurl\b.*\|\s*(ba)?sh\b", "pipe to shell"),
    (r"\bwget\b.*\|\s*(ba)?sh\b", "pipe to shell"),
    (r"\bcurl\b.*\|\s*sudo\b", "pipe to sudo"),
    (r"\beval\b.*\$\(", "eval with command substitution"),
    (r"\bbase64\s+(-d|--decode)\b.*\|\s*(eval|sh|bash)", "obfuscated execution"),
    (r"\bgit\s+push\s+.*--force\b(?!-with-lease)", "force push"),
    (r"\bgit\s+reset\s+--hard\b", "hard reset"),
    (r"\bgit\s+checkout\s+--\s*\.", "discard all local changes"),
    (r"\bgit\s+filter-repo\b", "history rewriting"),
    (r"\bgit\s+rebase\b.*-x\b", "automated rebase execution"),
    (r"\bGIT_SEQUENCE_EDITOR\b.*\bgit\s+rebase\b", "automated interactive rebase"),
    (r"\bmkfs\b", "filesystem creation"),
    (r"\bdd\s+.*\bof=/dev/", "raw disk write"),
    (r"\bsudo\s+rm\s+-rf\b", "sudo recursive delete"),
    (r"\bsudo\s+chmod\b.*\b(777|666)\b", "sudo insecure permissions"),
    (r"\brm\s+-rf\s+(/|~|\$HOME)\s*$", "recursive delete of root/home"),
    (r"\brm\s+-rf\s+/(?:usr|etc|var|System|Library|bin|sbin|opt)\b", "recursive delete of system dir"),
    (r"\bchmod\s+-R\s+777\s+/(?:usr|etc|var|Library|System)\b", "insecure permissions on system dir"),
    (r"\bsudo\s+chmod\s+-R\b", "sudo recursive permissions change"),
]

NEEDS_APPROVAL_PATTERNS = [
    (r"\bmkdir\b", "directory creation"),
    (r"\bmv\b", "file move"),
    (r"\bcp\b", "file copy"),
    (r"\brm\b", "file deletion"),
    (r"\bsed\s+-i\b", "in-place file edit"),
    (r"\bperl\s+-[ip]", "in-place file edit"),
    (r"\bcargo\s+(build|run|install|test|bench|publish)\b", "cargo execution"),
    (r"\bcargo\s+fmt\b", "cargo format"),
    (r"\bnpm\s+(install|run|start|build|publish|exec)\b", "npm execution"),
    (r"\bbun\s+(install|run|start|build|add|remove|x)\b", "bun execution"),
    (r"\bbunx\b", "bun execution"),
    (r"\bpip\s+install\b", "pip install"),
    (r"\buv\s+(run|pip|sync|lock|add|remove)\b", "uv execution"),
    (r"\buvx\b", "uv execution"),
    (r"\bpython3?\b", "python execution"),
    (r"\bnode\b", "node execution"),
    (r"\bjust\b", "task runner execution"),
    (r"\bmake\b", "make execution"),
    (r"\bnix\s+(build|develop|run|shell)\b", "nix execution"),
    (r"\bdocker\b", "docker operation"),
    (r"\bpodman\b", "container operation"),
    (r"\bkubectl\b", "kubernetes operation"),
    (r"\bansible\b", "ansible operation"),
    (r"\bssh\b", "remote connection"),
    (r"\bscp\b", "remote file copy"),
    (r"\brsync\b", "file sync"),
    (r"\bcurl\b", "network request"),
    (r"\bwget\b", "network download"),
    (r"\bhttpie\b|\bhttp\s", "network request"),
    (r"\bkill\b|\bpkill\b|\bkillall\b", "process management"),
    (r"\bchmod\b", "permission change"),
    (r"\bchown\b", "ownership change"),
    (r"\bln\b", "link creation"),
    (r"\btouch\b", "file creation"),
    (r"\btee\b", "file write via tee"),
    (r"\bgit\s+push\b", "git push"),
    (r"\bgit\s+tag\s+-[fd]", "git tag modification"),
    (r"\bgit\s+branch\s+-[dD]", "git branch deletion"),
    (r"\bgit\s+reset\b", "git reset"),
    (r"\bgit\s+checkout\b", "git checkout"),
    (r"\bbrew\s+(install|upgrade|uninstall|update|tap|untap)\b", "homebrew operation"),
    (r"\bcodesign\b", "code signing"),
    (r"\bxcodebuild\b", "xcode build"),
    (r"\bswift\s+(build|run|test|package)\b", "swift execution"),
    (r"\bgh\s+(pr|issue|release|repo)\s+(create|edit|close|delete|merge)\b", "github write operation"),
    (r"\bffmpeg\b", "media processing"),
    (r"\bpinchtab\b", "browser automation"),
    (r">\s*[^&]", "file redirect/write"),
    (r">>\s*", "file append"),
    (r"\bsudo\b", "privilege escalation"),
    (r"\badb\b", "android debug"),
    (r"\bcpio\b", "archive operation"),
    (r"\btar\s+.*[xc]", "archive operation"),
    (r"\bunzip\b|\bzip\b|\bgzip\b|\bgunzip\b", "archive operation"),
    (r"\bpkgutil\b", "package utility"),
    (r"\bcolima\b", "VM operation"),
]

SAFE_PATTERNS = [
    (r"^\s*#", "comment line"),
    (r"^\s*$", "empty command"),
]

WELL_KNOWN_DOMAINS = {
    "github.com", "gitlab.com", "bitbucket.org",
    "pypi.org", "npmjs.com", "npmjs.org", "crates.io",
    "hub.docker.com", "docker.io",
    "stackoverflow.com", "google.com",
    "rust-lang.org", "python.org", "nodejs.org",
    "localhost", "127.0.0.1", "0.0.0.0",
    "host.example.com", "example.com", "example.org", "example.net",
    "host.example.space", "panel.example.space",
}

IP_PATTERN = re.compile(r"\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b")
DOMAIN_PATTERN = re.compile(r"\b([a-zA-Z0-9][-a-zA-Z0-9]*(?:\.[a-zA-Z0-9][-a-zA-Z0-9]*)+)\b")


def detect_pii(command: str) -> list[str]:
    pii = []
    for ip in IP_PATTERN.findall(command):
        if ip not in ("127.0.0.1", "0.0.0.0") and not ip.startswith("192.168.") and not ip.startswith("10.") and not ip.startswith("172."):
            pii.append(f"{ip} → 0.0.0.0")
    for domain in DOMAIN_PATTERN.findall(command):
        base = domain.lower()
        if base in WELL_KNOWN_DOMAINS:
            continue
        if any(base.endswith(f".{wk}") for wk in WELL_KNOWN_DOMAINS):
            continue
        if base.endswith(".example.com") or base.endswith(".example.space") or base.endswith(".example.org"):
            continue
        tld = base.rsplit(".", 1)[-1]
        if tld in ("com", "org", "net", "io", "dev", "app", "space", "cloud", "me", "sh", "co", "ru", "uk", "de"):
            pii.append(f"{domain} → host.example.com")
    return pii


def classify_command(command: str) -> tuple[str, int, str]:
    cmd = command.strip()
    if not cmd:
        return "safe", 5, "empty command"

    for pattern, reason in SAFE_PATTERNS:
        if re.search(pattern, cmd):
            return "safe", 5, reason

    for pattern, reason in DANGEROUS_PATTERNS:
        if re.search(pattern, cmd, re.IGNORECASE):
            return "dangerous", 4, reason

    # Docker destructive
    if re.search(r"\bdocker\s+system\s+prune\b.*-a", cmd):
        return "dangerous", 4, "docker full purge"
    if re.search(r"\bcolima\s+(delete|stop)\b.*--force", cmd):
        return "dangerous", 4, "forced VM deletion"

    # Git subcommand classification
    git_match = re.search(r"\bgit\s+(\S+)", cmd)
    if git_match:
        sub = git_match.group(1)
        # git push --force-with-lease is needs-approval (safer than --force)
        if sub == "push" and "--force-with-lease" in cmd and "--force" not in cmd.replace("--force-with-lease", ""):
            return "needs-approval", 4, "git push with force-with-lease (bounded force)"
        if sub in SAFE_GIT_SUBCOMMANDS:
            # But git branch -D is needs-approval
            if sub == "branch" and re.search(r"-[dD]", cmd):
                return "needs-approval", 4, "git branch deletion"
            if sub == "tag" and re.search(r"-[fd]", cmd):
                return "needs-approval", 4, "git tag modification"
            if sub == "config" and not re.search(r"--get|--list|--show", cmd) and "=" in cmd:
                return "needs-approval", 3, "git config write"
            if sub == "stash" and re.search(r"\b(drop|clear|pop)\b", cmd):
                return "needs-approval", 4, "git stash modification"
            return "safe", 5, f"read-only git {sub}"
        if sub in NEEDS_APPROVAL_GIT_SUBCOMMANDS:
            return "needs-approval", 4, f"git {sub}"

    # First token analysis
    tokens = cmd.split()
    first = tokens[0].split("/")[-1] if tokens else ""
    # Strip env vars prefix like FOO=bar command
    while "=" in first and len(tokens) > 1:
        tokens = tokens[1:]
        first = tokens[0].split("/")[-1] if tokens else ""

    if first in SAFE_FIRST_TOKENS:
        # But check for write redirects
        if ">" in cmd and first in ("echo", "printf", "cat"):
            return "needs-approval", 4, f"{first} with file redirect"
        return "safe", 5, f"read-only {first} command"

    # Cargo/npm read-only
    if first == "cargo" and len(tokens) > 1 and tokens[1] in ("check", "clippy", "doc", "metadata", "tree", "verify-project"):
        return "safe", 4, f"read-only cargo {tokens[1]}"

    if first == "npm" and len(tokens) > 1 and tokens[1] in ("list", "ls", "view", "info", "outdated", "audit"):
        return "safe", 4, f"read-only npm {tokens[1]}"

    if first == "gh" and len(tokens) > 1:
        gh_sub = " ".join(tokens[1:3]) if len(tokens) > 2 else tokens[1]
        if any(r in gh_sub for r in ("list", "view", "status", "checks")):
            return "safe", 4, f"read-only gh {gh_sub}"

    if first == "brew" and len(tokens) > 1 and tokens[1] in ("list", "info", "search", "leaves", "deps", "doctor", "config"):
        return "safe", 4, f"read-only brew {tokens[1]}"

    if first == "nix" and len(tokens) > 1 and tokens[1] in ("eval", "show-config", "doctor", "path-info", "store"):
        return "safe", 4, f"read-only nix {tokens[1]}"

    # Pipe chains: check if the final command is safe
    if "|" in cmd:
        pipe_parts = cmd.split("|")
        last_part = pipe_parts[-1].strip().split()[0].split("/")[-1] if pipe_parts[-1].strip() else ""
        # If everything is reads piped together, it's safe
        all_read = all(
            p.strip().split()[0].split("/")[-1] in SAFE_FIRST_TOKENS
            for p in pipe_parts
            if p.strip()
        )
        if all_read and ">" not in cmd:
            return "safe", 4, "read-only pipe chain"

    # Needs-approval patterns
    for pattern, reason in NEEDS_APPROVAL_PATTERNS:
        if re.search(pattern, cmd, re.IGNORECASE):
            conf = 4
            # sudo bumps concern
            if re.search(r"\bsudo\b", cmd) and reason != "privilege escalation":
                return "dangerous" if any(d in cmd for d in ("rm", "chmod", "chown", "dd", "mkfs")) else "needs-approval", 3, f"sudo + {reason}"
            return "needs-approval", conf, reason

    # Unknown commands default to needs-approval
    return "needs-approval", 3, f"unknown command: {first}"


def process_range(input_path: Path, output_path: Path, start: int, end: int):
    with open(input_path) as f:
        lines = f.readlines()

    # start/end are 1-indexed inclusive
    chunk = lines[start - 1 : end]
    results = []

    for line in chunk:
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
            command = obj["command"]
        except (json.JSONDecodeError, KeyError):
            continue

        label, confidence, reason = classify_command(command)
        pii = detect_pii(command)

        results.append(json.dumps({
            "command": command,
            "label": label,
            "confidence": confidence,
            "reason": reason,
            "pii": pii,
        }, ensure_ascii=False))

    output_path.parent.mkdir(parents=True, exist_ok=True)
    with open(output_path, "w") as f:
        f.write("\n".join(results) + "\n" if results else "")

    return len(results)


def main():
    if len(sys.argv) < 5:
        print(f"Usage: {sys.argv[0]} <input> <output> <start> <end>")
        sys.exit(1)

    input_path = Path(sys.argv[1])
    output_path = Path(sys.argv[2])
    start = int(sys.argv[3])
    end = int(sys.argv[4])

    count = process_range(input_path, output_path, start, end)
    print(f"Wrote {count} labeled commands to {output_path}")


if __name__ == "__main__":
    main()
