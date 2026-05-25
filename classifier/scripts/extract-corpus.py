# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Extract bash commands from the shparse test corpus.

Parses the === / --- / --- test format and emits commands as JSONL.

Usage:
    uv run scripts/extract-corpus.py /path/to/tests/corpus -o data/corpus-commands.jsonl
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path


def extract_from_file(path: Path) -> list[str]:
    commands: list[str] = []
    lines = path.read_text().splitlines()
    i = 0
    while i < len(lines):
        if lines[i].startswith("=== "):
            i += 1
            cmd_lines: list[str] = []
            while i < len(lines) and lines[i] != "---":
                cmd_lines.append(lines[i])
                i += 1
            cmd = "\n".join(cmd_lines).strip()
            if cmd:
                commands.append(cmd)
        i += 1
    return commands


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("corpus_dir", type=Path)
    parser.add_argument("-o", "--output", type=Path, default=Path("data/corpus-commands.jsonl"))
    args = parser.parse_args()

    commands: list[str] = []
    seen: set[str] = set()

    for path in sorted(args.corpus_dir.glob("*.tests")):
        for cmd in extract_from_file(path):
            if cmd not in seen:
                seen.add(cmd)
                commands.append(cmd)

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w") as f:
        for cmd in commands:
            f.write(json.dumps({"command": cmd, "source": "shparse-corpus"}) + "\n")

    print(f"Extracted {len(commands)} unique commands → {args.output}", file=sys.stderr)


if __name__ == "__main__":
    main()
