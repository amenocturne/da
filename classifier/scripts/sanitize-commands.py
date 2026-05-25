# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Sanitize extracted commands for public release.

Replaces personal identifiers (usernames, hostnames, IPs, paths, tokens)
with generic placeholders. Filters out non-bash content.

Usage:
    uv run scripts/sanitize-commands.py -i data/commands.jsonl -o data/commands-public.jsonl
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

NOT_BASH = re.compile(
    r"^(import |from \S+ import |def |class |#!.*python|"
    r"async def |await |raise |try:|except |finally:)",
    re.MULTILINE,
)

FILE_EXTENSIONS = (
    "rs", "py", "ts", "tsx", "js", "jsx", "json", "yaml", "yml", "toml",
    "md", "txt", "html", "css", "scss", "sh", "bash", "zsh", "fish",
    "lock", "log", "csv", "xml", "conf", "cfg", "ini", "env", "gitignore",
    "h", "c", "cpp", "go", "rb", "lua", "zig", "nix", "flake", "sql",
    "wasm", "whl", "tar", "gz", "zip", "pkg", "dmg", "app", "dylib",
    "so", "o", "a", "bin", "exe", "plist", "bak", "tmp", "swp", "onnx",
    "pt", "safetensors", "mlpackage", "wav", "mp3", "mp4", "flac", "ogg",
    "png", "jpg", "jpeg", "gif", "svg", "pdf", "webp", "ico",
)

FILTERS = [
    lambda cmd: bool(cmd.strip()),
    lambda cmd: not NOT_BASH.match(cmd),
    lambda cmd: len(cmd) < 5000,
]


def build_replacements(extra_map: dict[str, str] | None = None) -> list[tuple[re.Pattern, str]]:
    rules: list[tuple[re.Pattern, str]] = []

    if extra_map:
        for pattern, replacement in extra_map.items():
            rules.append((re.compile(re.escape(pattern), re.IGNORECASE), replacement))

    rules.extend([
        (re.compile(r"/Users/\w+"), "/Users/USER"),
        (re.compile(r"/home/\w+"), "/home/USER"),
        (re.compile(r"~"), "/Users/USER"),

        (re.compile(
            r"\b\w+@(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?\.)"
            r"+[a-zA-Z]{2,}\b"
        ), "user@host.example.com"),

        (re.compile(
            r"(?<=://)(?:[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?\.)"
            r"+[a-zA-Z]{2,}\b"
        ), "host.example.com"),

        (re.compile(r"\b\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}\b"), "0.0.0.0"),

        (re.compile(
            r"\b[a-zA-Z0-9_-]{20,}(?:key|token|secret|password|api|auth)[a-zA-Z0-9_-]*\b",
            re.IGNORECASE,
        ), "REDACTED_TOKEN"),
        (re.compile(
            r"\b(?:key|token|secret|password|api_key|auth)=\S+",
            re.IGNORECASE,
        ), r"TOKEN=REDACTED"),

        (re.compile(
            r"(?:ssh|scp|rsync)\s+(?:.*\s)?(\w+)@"
        ), lambda m: m.group(0).replace(m.group(1) + "@", "USER@")),
    ])

    return rules


def sanitize(cmd: str, rules: list[tuple[re.Pattern, str]]) -> str:
    for pattern, repl in rules:
        if callable(repl):
            cmd = pattern.sub(repl, cmd)
        else:
            cmd = pattern.sub(repl, cmd)
    return cmd


def is_bash(cmd: str) -> bool:
    return all(f(cmd) for f in FILTERS)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("-i", "--input", type=Path, required=True)
    parser.add_argument("-o", "--output", type=Path, required=True)
    parser.add_argument(
        "--extra-map", type=Path, default=None,
        help="JSON file with extra string→replacement mappings",
    )
    parser.add_argument(
        "--strip-metadata", action="store_true",
        help="Remove session_id, timestamp, cwd (keep only command)",
    )
    args = parser.parse_args()

    extra_map = None
    if args.extra_map and args.extra_map.exists():
        extra_map = json.loads(args.extra_map.read_text())

    rules = build_replacements(extra_map)

    kept = 0
    filtered = 0

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.input.open() as fin, args.output.open("w") as fout:
        for line in fin:
            rec = json.loads(line)

            if not is_bash(rec["command"]):
                filtered += 1
                continue

            rec["command"] = sanitize(rec["command"], rules)
            rec["cwd"] = sanitize(rec.get("cwd", ""), rules)

            if args.strip_metadata:
                rec = {"command": rec["command"]}

            fout.write(json.dumps(rec) + "\n")
            kept += 1

    print(f"Kept {kept}, filtered {filtered} → {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
